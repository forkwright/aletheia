//! Memory-mapped vector storage with runtime-switchable access hints.
//!
//! Stores dense vectors in a flat file, memory-mapped for zero-copy access.
//! Supports two access patterns switchable at runtime:
//!
//! - **Sequential** (`MADV_SEQUENTIAL`): optimal for indexing passes that scan
//!   all vectors linearly.
//! - **Random** (`MADV_RANDOM`): optimal for search queries that access vectors
//!   in unpredictable order.
//!
//! Uses `rustix::mm` for mmap and madvise (Linux/macOS). Falls back to heap
//! storage on non-Unix platforms or when the file is empty.
#![expect(
    dead_code,
    reason = "infrastructure for future HNSW storage integration"
)]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use tracing::debug;

use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;

/// Borrow the raw fd from a `std::fs::File` as a `rustix::fd::BorrowedFd`.
#[cfg(unix)]
fn borrow_fd(file: &File) -> rustix::fd::BorrowedFd<'_> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: `file` is an open `std::fs::File`, so `as_raw_fd()` returns a
    // valid fd. The returned `BorrowedFd` borrows `file` (via the `'_` lifetime
    // tied to the function parameter), preventing use-after-close.
    #[expect(
        unsafe_code,
        reason = "BorrowedFd::borrow_raw is the documented path to go from std::fs::File to rustix fd APIs"
    )]
    unsafe {
        rustix::fd::BorrowedFd::borrow_raw(file.as_raw_fd())
    }
}

/// Access hint for mmap advisory calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum AccessHint {
    /// Sequential access -- best for full-scan indexing passes.
    Sequential = 0,
    /// Random access -- best for search queries.
    Random = 1,
}

impl From<AccessHint> for u8 {
    fn from(hint: AccessHint) -> Self {
        match hint {
            AccessHint::Sequential => 0,
            AccessHint::Random => 1,
        }
    }
}

/// Memory-mapped vector storage.
///
/// Vectors are stored contiguously as `[f32; dim]` arrays. Each vector is
/// `dim * 4` bytes. Vectors are addressed by a zero-based index.
pub(crate) struct MmapVectorStorage {
    path: PathBuf,
    dim: usize,
    /// Number of vectors currently stored.
    count: usize,
    /// Current access hint (atomic for lock-free switching).
    hint: AtomicU8,
    /// Backing file handle (kept open for appends and remaps).
    file: File,
    /// The storage backend -- mmap on Unix, heap buffer elsewhere.
    inner: StorageInner,
}

enum StorageInner {
    /// Memory-mapped region (Unix only).
    #[cfg(unix)]
    Mmap {
        /// Pointer to the mmap region.
        ptr: *mut u8,
        /// Length of the mapped region in bytes.
        len: usize,
    },
    /// Heap-allocated fallback (non-Unix or empty file).
    Heap(Vec<u8>),
}

// SAFETY: `StorageInner` contains a raw `*mut u8` (the mmap pointer) which makes
// it `!Send + !Sync` by default. We assert both manually under the following
// invariants, all of which are structurally enforced by the public API on the
// owning `MmapVectorStorage`:
//
// 1. The pointer is set exactly twice: once in `create_inner` (during `open`,
//    which returns an owned value before any sharing is possible) and once in
//    `remap` (which takes `&mut self` on the owning storage). Both mutation
//    points require unique access, so no thread can observe a torn pointer.
//
// 2. Read access goes through `as_bytes()`, which takes `&self` on the owning
//    storage. Rust's aliasing rules guarantee that no `&mut self` call (i.e.
//    `push` / `remap`) can overlap with any `&self` read, so the pointer is
//    always either being uniquely updated or being shared for read.
//
// 3. The mmap region is backed by `MAP_SHARED` with `PROT_READ | PROT_WRITE`.
//    Concurrent reads from multiple threads through `&self` are sound because
//    (a) the kernel provides cache coherence for the mapped pages and (b) the
//    only writer path is `push`, which goes through `write_at` on the backing
//    `File` and then calls `remap` under `&mut self` — so the readable window
//    grows monotonically and no reader ever observes a partially-written
//    vector.
//
// 4. On `Drop`, `munmap` is called while the value is uniquely owned, so no
//    other thread holds a reference to the pointer.
//
// `Send` is sound for the same reason: the owning `MmapVectorStorage` enforces
// exclusive access during mutation, so transferring ownership between threads
// never races with an active borrower.
#[expect(
    unsafe_code,
    reason = "raw *mut u8 defaults to !Send; soundness documented in the SAFETY block above (4 invariants)"
)]
unsafe impl Send for StorageInner {}
#[expect(
    unsafe_code,
    reason = "raw *mut u8 defaults to !Sync; soundness documented in the SAFETY block above (4 invariants)"
)]
unsafe impl Sync for StorageInner {}

impl Drop for StorageInner {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let StorageInner::Mmap { ptr, len } = *self
            && len > 0
        {
            // SAFETY: Invariants for `rustix::mm::munmap`:
            //   1. `ptr` and `len` come from a successful `mmap` (in
            //      `create_inner` or `remap`) and are stored in
            //      `StorageInner::Mmap { ptr, len }` as a coherent pair.
            //   2. `Drop` runs when the value is uniquely owned (by the Rust
            //      borrow checker), so no other thread holds a reference to
            //      this mapping — see invariant #4 of the `Send`/`Sync` SAFETY
            //      block above.
            //   3. After `munmap`, the struct is dropped, so the `ptr`/`len`
            //      fields cannot be read again (use-after-unmap is impossible).
            //   4. `munmap` failure during `Drop` is non-recoverable (we cannot
            //      propagate an error out of `Drop`); `.ok()` is deliberate.
            #[expect(
                unsafe_code,
                reason = "paired with the mmap calls in create_inner/remap; Drop runs under unique ownership"
            )]
            unsafe {
                rustix::mm::munmap(ptr.cast(), len).ok();
            }
        }
    }
}

fn io_err(op: &str, reason: String) -> crate::error::InternalError {
    crate::error::InternalError::Runtime {
        source: InvalidOperationSnafu { op, reason }.build(),
    }
}

impl MmapVectorStorage {
    /// Open or create a vector storage file at `path` with the given dimension.
    ///
    /// If the file exists, it is opened and mapped. The vector count is inferred
    /// from `file_size / (dim * 4)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file size is not a multiple of `dim * 4` bytes,
    /// or if `dim` is zero.
    pub(crate) fn open(path: impl AsRef<Path>, dim: usize) -> Result<Self> {
        if dim == 0 {
            return Err(InvalidOperationSnafu {
                op: "mmap_storage",
                reason: "vector dimension must be > 0".to_string(),
            }
            .build()
            .into());
        }

        let path = path.as_ref().to_path_buf();
        let stride = dim * std::mem::size_of::<f32>();

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| {
                io_err(
                    "mmap_storage",
                    format!("failed to open {}: {e}", path.display()),
                )
            })?;

        let file_len = file
            .metadata()
            .map_err(|e| {
                io_err(
                    "mmap_storage",
                    format!("failed to stat {}: {e}", path.display()),
                )
            })?
            .len();

        #[expect(
            clippy::cast_possible_truncation,
            reason = "file size bounded by available memory"
        )]
        let file_len_usize = file_len as usize;

        if !file_len_usize.is_multiple_of(stride) {
            return Err(InvalidOperationSnafu {
                op: "mmap_storage",
                reason: format!(
                    "file size {} is not a multiple of vector stride {}",
                    file_len, stride
                ),
            }
            .build()
            .into());
        }

        let count = file_len_usize / stride;
        let inner = Self::create_inner(&file, file_len_usize)?;
        let hint = AtomicU8::new(u8::from(AccessHint::Random));

        debug!(path = %path.display(), dim, count, "opened mmap vector storage");

        Ok(Self {
            path,
            dim,
            count,
            hint,
            file,
            inner,
        })
    }

    #[cfg(unix)]
    fn create_inner(file: &File, len: usize) -> Result<StorageInner> {
        if len == 0 {
            return Ok(StorageInner::Heap(Vec::new()));
        }

        let fd = borrow_fd(file);
        // SAFETY: Invariants for `rustix::mm::mmap`:
        //   1. `addr = null_mut()` — kernel picks the mapping location, so we
        //      do not clobber an existing mapping.
        //   2. `len > 0` — caller short-circuits the `len == 0` case above; a
        //      zero-length mmap is EINVAL on Linux.
        //   3. `prot = READ | WRITE` — matches the `OpenOptions::new().read(true).write(true)`
        //      on `file`, so the kernel will grant the requested access.
        //   4. `flags = SHARED` — required for `msync`/`set_len` to propagate to
        //      the backing file; no `ANONYMOUS` or `PRIVATE` means the file
        //      descriptor is the authority for the mapping's contents.
        //   5. `fd` is a live `BorrowedFd<'_>` whose lifetime outlives this
        //      syscall (it borrows from the caller-owned `file`).
        //   6. `offset = 0` — the entire file is mapped; offset is page-aligned.
        // Returned pointer is stored in `StorageInner::Mmap` and unmapped via
        // `munmap` in `Drop` (site #4).
        #[expect(
            unsafe_code,
            reason = "rustix::mm::mmap is the FFI for establishing a file-backed memory mapping; 6 invariants enumerated above"
        )]
        let ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                len,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                fd,
                0,
            )
        }
        .map_err(|e| io_err("mmap_storage", format!("mmap failed: {e}")))?;

        Ok(StorageInner::Mmap {
            ptr: ptr.cast(),
            len,
        })
    }

    #[cfg(not(unix))]
    fn create_inner(file: &File, len: usize) -> Result<StorageInner> {
        use std::io::Read;

        if len == 0 {
            return Ok(StorageInner::Heap(Vec::new()));
        }

        let mut buf = vec![0u8; len];
        let mut f = file
            .try_clone()
            .map_err(|e| io_err("mmap_storage", format!("file clone failed: {e}")))?;
        f.read_exact(&mut buf)
            .map_err(|e| io_err("mmap_storage", format!("read failed: {e}")))?;
        Ok(StorageInner::Heap(buf))
    }

    /// Remap the file after a size change (Unix only).
    #[cfg(unix)]
    fn remap(&mut self, new_len: usize) -> Result<()> {
        // Replace inner with a placeholder Heap to take ownership of the old Mmap.
        // The old value's Drop will call munmap.
        let _old = std::mem::replace(&mut self.inner, StorageInner::Heap(Vec::new()));
        // _old is dropped here, which munmaps the old mapping if it was Mmap.

        if new_len == 0 {
            return Ok(());
        }

        let fd = borrow_fd(&self.file);
        // SAFETY: Same invariants as `create_inner`'s `mmap` (see site above):
        // null addr, non-zero len, READ|WRITE matches file mode, SHARED flag,
        // live BorrowedFd, offset 0. Additionally, the previous mapping was
        // released by the `std::mem::replace` + `Drop` sequence a few lines
        // above, so this syscall never double-maps the same file region.
        #[expect(
            unsafe_code,
            reason = "rustix::mm::mmap is the FFI for establishing a file-backed memory mapping; invariants match create_inner"
        )]
        let ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                new_len,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                fd,
                0,
            )
        }
        .map_err(|e| io_err("mmap_storage", format!("remap failed: {e}")))?;

        self.inner = StorageInner::Mmap {
            ptr: ptr.cast(),
            len: new_len,
        };

        // Re-apply current access hint.
        let hint = match self.hint.load(Ordering::Relaxed) {
            0 => AccessHint::Sequential,
            _ => AccessHint::Random,
        };
        self.apply_madvise(hint);

        Ok(())
    }

    /// Switch the access hint at runtime.
    ///
    /// On Unix this calls `madvise` to inform the kernel. On other platforms
    /// this is a no-op (the hint is recorded but not applied).
    pub(crate) fn set_access_hint(&self, hint: AccessHint) {
        self.hint.store(u8::from(hint), Ordering::Relaxed);
        self.apply_madvise(hint);
    }

    #[cfg(unix)]
    fn apply_madvise(&self, hint: AccessHint) {
        if let StorageInner::Mmap { ptr, len } = self.inner {
            if len == 0 {
                return;
            }
            let advice = match hint {
                AccessHint::Sequential => rustix::mm::Advice::Sequential,
                AccessHint::Random => rustix::mm::Advice::Random,
            };
            // SAFETY: Invariants for `rustix::mm::madvise`:
            //   1. `ptr` + `len` came from a live mmap (still in
            //      `StorageInner::Mmap`) and are paired coherently.
            //   2. `len > 0` is checked on the line above; zero-length madvise
            //      would be EINVAL but has no correctness impact here.
            //   3. `madvise` is purely advisory — a failure does not invalidate
            //      the mapping, does not touch memory, and does not affect
            //      correctness of subsequent reads/writes. `.ok()` is correct.
            //   4. This runs under `&self`; no other thread can be inside
            //      `&mut self` (`remap`/`push`), so `ptr`/`len` cannot be
            //      swapped under us.
            #[expect(
                unsafe_code,
                reason = "madvise is a hint-only FFI call; mmap region validity documented above"
            )]
            unsafe {
                rustix::mm::madvise(ptr.cast(), len, advice).ok();
            }
        }
    }

    #[cfg(not(unix))]
    fn apply_madvise(&self, _hint: AccessHint) {
        // No-op on non-Unix.
    }

    /// Current access hint.
    pub(crate) fn access_hint(&self) -> AccessHint {
        match self.hint.load(Ordering::Relaxed) {
            0 => AccessHint::Sequential,
            _ => AccessHint::Random,
        }
    }

    /// Number of vectors stored.
    pub(crate) fn len(&self) -> usize {
        self.count
    }

    /// Whether the storage is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn as_bytes(&self) -> &[u8] {
        match &self.inner {
            #[cfg(unix)]
            StorageInner::Mmap { ptr, len } => {
                if *len == 0 {
                    return &[];
                }
                // SAFETY: Invariants for `slice::from_raw_parts`:
                //   1. `ptr` is non-null and points to `len` contiguous bytes —
                //      it was returned by a successful `mmap` in `create_inner`
                //      or `remap` (both sites above) and stored in
                //      `StorageInner::Mmap { ptr, len }`. The variant enforces
                //      the pair stays coherent: we never mutate `ptr` or `len`
                //      independently; the Mmap struct is replaced atomically in
                //      `remap` via `std::mem::replace`.
                //   2. The bytes are initialized — mmap of a regular file with
                //      `MAP_SHARED` returns initialized memory (the file's
                //      contents, including any set_len extension which the
                //      kernel zero-fills).
                //   3. The region is valid for reads for the duration of the
                //      returned `&[u8]` — the `&self` borrow on `MmapVectorStorage`
                //      prevents any concurrent `remap` (which requires `&mut
                //      self`), so the mapping cannot be unmapped mid-borrow.
                //   4. `len <= isize::MAX` — enforced by the filesystem/OS cap
                //      on mmap size; a larger mapping would have failed at the
                //      mmap syscall.
                //   5. No alias to a `&mut` slice exists — this struct never
                //      hands out `&mut` into the mapped region; writes go
                //      through `File::write_at` + `remap`, not through a
                //      `&mut [u8]` slice.
                #[expect(
                    unsafe_code,
                    reason = "slice::from_raw_parts is the only way to expose mmap memory as &[u8]; 5 invariants documented above"
                )]
                unsafe {
                    std::slice::from_raw_parts(*ptr, *len)
                }
            }
            StorageInner::Heap(buf) => buf,
        }
    }

    /// Read vector at `index` as a slice of `f32`.
    ///
    /// Returns `None` if `index >= count`.
    pub(crate) fn get(&self, index: usize) -> Option<&[f32]> {
        if index >= self.count {
            return None;
        }
        let stride = self.dim * std::mem::size_of::<f32>();
        let offset = index * stride;
        let bytes = &self.as_bytes()[offset..offset + stride];
        // bytemuck verifies alignment and size at runtime; mmap data written
        // from f32 slices is always properly aligned.
        let floats: &[f32] = bytemuck::cast_slice(bytes);
        Some(floats)
    }

    /// Append a vector to the storage.
    ///
    /// # Errors
    ///
    /// Returns an error if the vector dimension does not match, or if the write
    /// fails.
    pub(crate) fn push(&mut self, vector: &[f32]) -> Result<usize> {
        if vector.len() != self.dim {
            return Err(InvalidOperationSnafu {
                op: "mmap_storage",
                reason: format!(
                    "vector dimension mismatch: expected {}, got {}",
                    self.dim,
                    vector.len()
                ),
            }
            .build()
            .into());
        }

        let stride = self.dim * std::mem::size_of::<f32>();
        let bytes: &[u8] = bytemuck::cast_slice(vector);

        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            // INVARIANT: self.count * stride fits in usize (already materialized); on 64-bit
            // targets usize == u64 width, on 32-bit targets the conversion still saturates
            // cleanly because usize::MAX <= u64::MAX.
            let offset = u64::try_from(self.count * stride).unwrap_or(u64::MAX);
            self.file
                .write_at(bytes, offset)
                .map_err(|e| io_err("mmap_storage", format!("write failed: {e}")))?;
            let new_len = (self.count + 1) * stride;
            let new_len_u64 = u64::try_from(new_len).unwrap_or(u64::MAX);
            self.file
                .set_len(new_len_u64)
                .map_err(|e| io_err("mmap_storage", format!("set_len failed: {e}")))?;
            self.remap(new_len)?;
        }

        #[cfg(not(unix))]
        {
            if let StorageInner::Heap(buf) = &mut self.inner {
                buf.extend_from_slice(bytes);
            }
        }

        let idx = self.count;
        self.count += 1;
        Ok(idx)
    }

    /// Flush changes to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush fails.
    pub(crate) fn flush(&self) -> Result<()> {
        #[cfg(unix)]
        if let StorageInner::Mmap { ptr, len } = self.inner
            && len > 0
        {
            // SAFETY: Invariants for `rustix::mm::msync`:
            //   1. `ptr` + `len` are from a live mmap held in
            //      `StorageInner::Mmap` and paired coherently.
            //   2. `len > 0` checked above; zero-length msync is EINVAL.
            //   3. `MsyncFlags::SYNC` requests synchronous writeback — the
            //      call returns after dirty pages are in stable storage. This
            //      is a read-only operation from the mapping's perspective; it
            //      does not modify or invalidate the pages.
            //   4. `&self` borrow excludes any concurrent `&mut self` path
            //      (`remap`/`push`), so `ptr`/`len` cannot race with unmap.
            #[expect(
                unsafe_code,
                reason = "msync is an FFI call; mmap region validity documented above"
            )]
            unsafe {
                rustix::mm::msync(ptr.cast(), len, rustix::mm::MsyncFlags::SYNC)
                    .map_err(|e| io_err("mmap_storage", format!("msync failed: {e}")))?;
            }
        }

        #[cfg(not(unix))]
        if let StorageInner::Heap(buf) = &self.inner {
            use std::io::Write;
            let mut file = File::create(&self.path)
                .map_err(|e| io_err("mmap_storage", format!("create failed: {e}")))?;
            file.write_all(buf)
                .map_err(|e| io_err("mmap_storage", format!("write failed: {e}")))?;
        }

        Ok(())
    }

    /// Path to the underlying file.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_push_and_get() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 3)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=3 should not fail"));
        assert!(storage.is_empty(), "new storage must be empty");

        let v1 = [1.0f32, 2.0, 3.0];
        let v2 = [4.0f32, 5.0, 6.0];
        let idx1 = storage.push(&v1).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });
        let idx2 = storage.push(&v2).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });

        assert_eq!(idx1, 0, "first vector index");
        assert_eq!(idx2, 1, "second vector index");
        assert_eq!(storage.len(), 2, "count after two pushes");

        let got1 = storage
            .get(0)
            .unwrap_or_else(|| unreachable!("INVARIANT: index 0 exists after push"));
        assert_eq!(got1, &v1, "vector 0 roundtrip");

        let got2 = storage
            .get(1)
            .unwrap_or_else(|| unreachable!("INVARIANT: index 1 exists after push"));
        assert_eq!(got2, &v2, "vector 1 roundtrip");

        assert!(storage.get(2).is_none(), "out-of-bounds returns None");
    }

    #[test]
    fn access_hint_switching() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 2)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=2 should not fail"));
        storage.push(&[1.0, 2.0]).unwrap_or_else(|_| {
            unreachable!("INVARIANT: push with correct dimension should not fail")
        });

        assert_eq!(
            storage.access_hint(),
            AccessHint::Random,
            "default hint is Random"
        );

        storage.set_access_hint(AccessHint::Sequential);
        assert_eq!(storage.access_hint(), AccessHint::Sequential);

        storage.set_access_hint(AccessHint::Random);
        assert_eq!(storage.access_hint(), AccessHint::Random);
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 4)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=4 should not fail"));
        let result = storage.push(&[1.0, 2.0]);
        assert!(result.is_err(), "wrong dimension should error");
    }

    #[test]
    fn zero_dimension_rejected() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("vectors.bin");
        let result = MmapVectorStorage::open(&path, 0);
        assert!(result.is_err(), "zero dimension should error");
    }

    #[test]
    fn reopen_persists_data() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("vectors.bin");

        {
            let mut storage = MmapVectorStorage::open(&path, 2).unwrap_or_else(|_| {
                unreachable!("INVARIANT: valid path and dim=2 should not fail")
            });
            storage.push(&[1.0, 2.0]).unwrap_or_else(|_| {
                unreachable!("INVARIANT: push with correct dimension should not fail")
            });
            storage.push(&[3.0, 4.0]).unwrap_or_else(|_| {
                unreachable!("INVARIANT: push with correct dimension should not fail")
            });
            storage.flush().unwrap_or_else(|_| {
                unreachable!("INVARIANT: flush should not fail on valid storage")
            });
        }

        let storage = MmapVectorStorage::open(&path, 2)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=2 should not fail"));
        assert_eq!(storage.len(), 2, "persisted count");
        let got = storage
            .get(1)
            .unwrap_or_else(|| unreachable!("INVARIANT: index 1 exists after push"));
        assert_eq!(got, &[3.0f32, 4.0], "persisted vector roundtrip");
    }

    #[test]
    fn mmap_fallback_for_empty_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("empty.bin");
        let storage = MmapVectorStorage::open(&path, 4)
            .unwrap_or_else(|_| unreachable!("INVARIANT: valid path and dim=4 should not fail"));
        assert!(storage.is_empty(), "empty file yields empty storage");
        assert!(storage.get(0).is_none(), "no vectors in empty storage");
    }

    /// Stress test: 16 threads × 1000 reads against a shared `MmapVectorStorage`.
    ///
    /// WHY: `StorageInner` carries a manual `unsafe impl Sync`. This test is a
    /// runtime sanity check that concurrent `&self` reads through the Sync
    /// boundary produce consistent results and do not crash the process.
    /// Each thread reads every vector in the storage and asserts the payload
    /// matches the deterministic pattern written during setup.
    #[test]
    fn concurrent_reads_are_consistent() {
        const DIM: usize = 8;
        const NUM_VECTORS: usize = 64;
        const NUM_THREADS: usize = 16;
        const READS_PER_THREAD: usize = 1000;

        let dir = tempfile::tempdir().unwrap_or_else(|_| {
            unreachable!("INVARIANT: temp dir creation should not fail in tests")
        });
        let path = dir.path().join("stress.bin");
        let mut storage = MmapVectorStorage::open(&path, DIM).unwrap_or_else(|_| {
            unreachable!("INVARIANT: valid path and non-zero DIM should not fail")
        });

        // Deterministic pattern: vector[i][j] = (i * DIM + j) as f32
        for i in 0..NUM_VECTORS {
            #[expect(
                clippy::cast_precision_loss,
                reason = "test fixture uses small integers well within f32 mantissa range"
            )]
            let vec: Vec<f32> = (0..DIM).map(|j| (i * DIM + j) as f32).collect();
            storage.push(&vec).unwrap_or_else(|_| {
                unreachable!("INVARIANT: push with correct dimension should not fail")
            });
        }

        let storage_ref = &storage;
        std::thread::scope(|s| {
            for _ in 0..NUM_THREADS {
                s.spawn(move || {
                    for _ in 0..READS_PER_THREAD {
                        for i in 0..NUM_VECTORS {
                            let v = storage_ref.get(i).unwrap_or_else(|| unreachable!("INVARIANT: index within range of pushed vectors"));
                            assert_eq!(v.len(), DIM, "vector length stable under concurrent read");
                            for (j, &val) in v.iter().enumerate() {
                                #[expect(
                                    clippy::cast_precision_loss,
                                    reason = "test fixture uses small integers well within f32 mantissa range"
                                )]
                                let expected = (i * DIM + j) as f32;
                                assert!(
                                    (val - expected).abs() < f32::EPSILON,
                                    "vector[{i}][{j}] = {val}, expected {expected}"
                                );
                            }
                        }
                    }
                });
            }
        });
    }
}
