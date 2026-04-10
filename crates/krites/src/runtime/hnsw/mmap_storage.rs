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
#![expect(
    unsafe_code,
    reason = "mmap requires unsafe FFI calls via rustix::mm for memory-mapped I/O"
)]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use tracing::debug;

use crate::error::InternalResult as Result;
use crate::runtime::error::InvalidOperationSnafu;

/// Borrow the raw fd from a `std::fs::File` as a `rustix::fd::BorrowedFd`.
///
/// # Safety
///
/// The returned `BorrowedFd` must not outlive the `File`.
#[cfg(unix)]
fn borrow_fd(file: &File) -> rustix::fd::BorrowedFd<'_> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: the file is open and we borrow for the lifetime of `file`.
    unsafe { rustix::fd::BorrowedFd::borrow_raw(file.as_raw_fd()) }
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
unsafe impl Send for StorageInner {}
unsafe impl Sync for StorageInner {}

impl Drop for StorageInner {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let StorageInner::Mmap { ptr, len } = *self
            && len > 0
        {
            // SAFETY: ptr and len are from a successful mmap call.
            unsafe {
                rustix::mm::munmap(ptr.cast(), len).ok(); // WHY: munmap failure during Drop is non-recoverable
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
        // SAFETY: file is open read-write, offset 0, len matches file size.
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
        // SAFETY: file is open read-write, offset 0, new_len matches file size.
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
            // SAFETY: ptr and len are from a valid mmap region.
            unsafe {
                rustix::mm::madvise(ptr.cast(), len, advice).ok(); // WHY: madvise is a hint; failure does not affect correctness
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
                // SAFETY: ptr and len are from a valid mmap region.
                unsafe { std::slice::from_raw_parts(*ptr, *len) }
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
        // SAFETY: stride is always a multiple of 4 (dim * sizeof(f32)).
        // File data is written from f32 slices so alignment is preserved.
        let (prefix, floats, suffix) = unsafe { bytes.align_to::<f32>() };
        debug_assert!(
            prefix.is_empty() && suffix.is_empty(),
            "vector data must be f32-aligned"
        );
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
        // SAFETY: f32 slice → u8 slice of same memory, stride = dim * 4.
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(vector.as_ptr().cast::<u8>(), stride) };

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
            // SAFETY: ptr and len are from a valid mmap region.
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
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 3).unwrap_or_else(|_| unreachable!());
        assert!(storage.is_empty(), "new storage must be empty");

        let v1 = [1.0f32, 2.0, 3.0];
        let v2 = [4.0f32, 5.0, 6.0];
        let idx1 = storage.push(&v1).unwrap_or_else(|_| unreachable!());
        let idx2 = storage.push(&v2).unwrap_or_else(|_| unreachable!());

        assert_eq!(idx1, 0, "first vector index");
        assert_eq!(idx2, 1, "second vector index");
        assert_eq!(storage.len(), 2, "count after two pushes");

        let got1 = storage.get(0).unwrap_or_else(|| unreachable!());
        assert_eq!(got1, &v1, "vector 0 roundtrip");

        let got2 = storage.get(1).unwrap_or_else(|| unreachable!());
        assert_eq!(got2, &v2, "vector 1 roundtrip");

        assert!(storage.get(2).is_none(), "out-of-bounds returns None");
    }

    #[test]
    fn access_hint_switching() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 2).unwrap_or_else(|_| unreachable!());
        storage.push(&[1.0, 2.0]).unwrap_or_else(|_| unreachable!());

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
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("vectors.bin");
        let mut storage = MmapVectorStorage::open(&path, 4).unwrap_or_else(|_| unreachable!());
        let result = storage.push(&[1.0, 2.0]);
        assert!(result.is_err(), "wrong dimension should error");
    }

    #[test]
    fn zero_dimension_rejected() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("vectors.bin");
        let result = MmapVectorStorage::open(&path, 0);
        assert!(result.is_err(), "zero dimension should error");
    }

    #[test]
    fn reopen_persists_data() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("vectors.bin");

        {
            let mut storage = MmapVectorStorage::open(&path, 2).unwrap_or_else(|_| unreachable!());
            storage.push(&[1.0, 2.0]).unwrap_or_else(|_| unreachable!());
            storage.push(&[3.0, 4.0]).unwrap_or_else(|_| unreachable!());
            storage.flush().unwrap_or_else(|_| unreachable!());
        }

        let storage = MmapVectorStorage::open(&path, 2).unwrap_or_else(|_| unreachable!());
        assert_eq!(storage.len(), 2, "persisted count");
        let got = storage.get(1).unwrap_or_else(|| unreachable!());
        assert_eq!(got, &[3.0f32, 4.0], "persisted vector roundtrip");
    }

    #[test]
    fn mmap_fallback_for_empty_file() {
        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("empty.bin");
        let storage = MmapVectorStorage::open(&path, 4).unwrap_or_else(|_| unreachable!());
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

        let dir = tempfile::tempdir().unwrap_or_else(|_| unreachable!());
        let path = dir.path().join("stress.bin");
        let mut storage = MmapVectorStorage::open(&path, DIM).unwrap_or_else(|_| unreachable!());

        // Deterministic pattern: vector[i][j] = (i * DIM + j) as f32
        for i in 0..NUM_VECTORS {
            #[expect(
                clippy::cast_precision_loss,
                reason = "test fixture uses small integers well within f32 mantissa range"
            )]
            let vec: Vec<f32> = (0..DIM).map(|j| (i * DIM + j) as f32).collect();
            storage.push(&vec).unwrap_or_else(|_| unreachable!());
        }

        let storage_ref = &storage;
        std::thread::scope(|s| {
            for _ in 0..NUM_THREADS {
                s.spawn(move || {
                    for _ in 0..READS_PER_THREAD {
                        for i in 0..NUM_VECTORS {
                            let v = storage_ref.get(i).unwrap_or_else(|| unreachable!());
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
