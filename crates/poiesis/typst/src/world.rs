//! Minimal in-memory implementation of the Typst `World` trait.
//!
//! Adapted from `ergon_tools/sdr` (Cody's private code, freely adaptable per
//! issue #3450). Changes from the sdr original:
//! - source lives in memory (not on disk) so `render_typst` can be called
//!   without touching the filesystem;
//! - an optional `data.json` virtual file is synthesized so templates can load
//!   injected data via `json("data.json")`;
//! - fonts are discovered from standard system paths, same as sdr.
//!
//! WHY library not CLI: reproducibility and error fidelity. The library route
//! gives typed diagnostics with source spans; the CLI route requires writing
//! temp files, parsing stderr, and depends on `typst` being on `PATH`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use typst::WorldExt;
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// The virtual path for the injected JSON data file.
///
/// Templates may call `json("data.json")` to load the data passed to
/// [`crate::render_typst`].
pub(crate) const DATA_VPATH: &str = "data.json";

/// A loaded font face.
struct FontEntry {
    font: Font,
}

/// A typst `World` backed by an in-memory main source and an optional
/// in-memory data blob, with fonts discovered from the system.
pub(crate) struct TypstWorld {
    /// `FileId` of the main `.typ` source (in-memory).
    main: FileId,
    /// In-memory main source text.
    main_source: Source,

    /// `FileId` of the virtual `data.json` file, if data was provided.
    data_id: Option<FileId>,
    /// Raw bytes of the injected data file.
    data_bytes: Option<Bytes>,

    /// Typst standard library.
    library: LazyHash<Library>,
    /// Metadata about all discovered fonts.
    book: LazyHash<FontBook>,
    /// Loaded font faces; indexed by position in `book`.
    fonts: Vec<FontEntry>,

    /// Per-file source/bytes cache for a single compilation.
    // WHY: parking_lot::Mutex — World trait methods are synchronous and the
    // lock is never held across an await point, so the tokio lock would add
    // unnecessary overhead.
    cache: Mutex<HashMap<FileId, CacheSlot>>,
}

#[derive(Default)]
struct CacheSlot {
    source: Option<FileResult<Source>>,
    bytes: Option<FileResult<Bytes>>,
}

impl TypstWorld {
    /// Create a world from a source string and optional serialized data bytes.
    pub(crate) fn new(source: &str, data_bytes: Option<Vec<u8>>) -> Self {
        let main_vpath = VirtualPath::new("main.typ");
        let main = FileId::new(None, main_vpath);
        let main_source = Source::new(main, source.to_owned());

        let (data_id, data_bytes) = match data_bytes {
            Some(bytes) => {
                let id = FileId::new(None, VirtualPath::new(DATA_VPATH));
                (Some(id), Some(Bytes::new(bytes)))
            }
            None => (None, None),
        };

        let (book, fonts) = discover_fonts();

        Self {
            main,
            main_source,
            data_id,
            data_bytes,
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(self.main_source.clone());
        }

        let mut cache = self.cache.lock();
        if let Some(slot) = cache.get(&id)
            && let Some(ref r) = slot.source
        {
            return r.clone();
        }

        // No disk-backed sources: only the main file and data.json are known.
        let path = id.vpath().as_rootless_path().to_path_buf();
        let result: FileResult<Source> = Err(FileError::NotFound(path));
        cache.entry(id).or_default().source = Some(result.clone());
        result
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let (Some(data_id), Some(data)) = (self.data_id, self.data_bytes.as_ref())
            && id == data_id
        {
            return Ok(data.clone());
        }

        let mut cache = self.cache.lock();
        if let Some(slot) = cache.get(&id)
            && let Some(ref r) = slot.bytes
        {
            return r.clone();
        }

        let path = id.vpath().as_rootless_path().to_path_buf();
        let result: FileResult<Bytes> = Err(FileError::NotFound(path));
        cache.entry(id).or_default().bytes = Some(result.clone());
        result
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).map(|e| e.font.clone())
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        // WHY: typst requests today in local time (offset=None) or UTC+N (offset=Some).
        let date = if let Some(hours) = offset {
            let offset_secs = i32::try_from(hours.saturating_mul(3600)).ok()?;
            let tz_offset = jiff::tz::Offset::from_seconds(offset_secs).ok()?;
            let tz = jiff::tz::TimeZone::fixed(tz_offset);
            jiff::Timestamp::now().to_zoned(tz).date()
        } else {
            jiff::Zoned::now().date()
        };

        Datetime::from_ymd(
            i32::from(date.year()),
            u8::try_from(date.month()).ok()?,
            u8::try_from(date.day()).ok()?,
        )
    }
}

/// Format typst diagnostics into a human-readable string with source locations.
pub(crate) fn format_diagnostics(world: &TypstWorld, diagnostics: &[SourceDiagnostic]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for diag in diagnostics {
        let severity = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };

        // WHY: WorldExt::range handles both numbered and raw-range span encodings.
        let location = world
            .range(diag.span)
            .and_then(|range| {
                let id = diag.span.id()?;
                let source = world.source(id).ok()?;
                let line = source.lines().byte_to_line(range.start)? + 1;
                let col = source.lines().byte_to_column(range.start)? + 1;
                let path = id.vpath().as_rootless_path().display().to_string();
                Some(format!("{path}:{line}:{col}"))
            })
            .unwrap_or_else(|| "<unknown>".to_owned());

        let _ = writeln!(out, "{severity}: {location}: {}", diag.message);
        for hint in &diag.hints {
            let _ = writeln!(out, "  hint: {hint}");
        }
    }
    out
}

// ── Font discovery ────────────────────────────────────────────────────────────

fn discover_fonts() -> (FontBook, Vec<FontEntry>) {
    let dirs = system_font_dirs();
    let mut book = FontBook::new();
    let mut fonts = Vec::new();

    for dir in dirs {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        scan_font_dir(read_dir, &mut book, &mut fonts);
    }

    tracing::debug!(count = fonts.len(), "discovered system fonts");
    (book, fonts)
}

fn scan_font_dir(read_dir: std::fs::ReadDir, book: &mut FontBook, fonts: &mut Vec<FontEntry>) {
    for entry in read_dir.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if let Ok(sub) = std::fs::read_dir(&path) {
                scan_font_dir(sub, book, fonts);
            }
            continue;
        }

        if !is_font_file(&path) {
            continue;
        }

        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let bytes = Bytes::new(data);

        for font in Font::iter(bytes) {
            book.push(font.info().clone());
            fonts.push(FontEntry { font });
        }
    }
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ttf" | "otf" | "ttc" | "otc" | "woff" | "woff2")
    )
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];

    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".fonts"));
        dirs.push(home.join(".local/share/fonts"));
    }

    // macOS system font directories.
    dirs.push(PathBuf::from("/Library/Fonts"));
    dirs.push(PathBuf::from("/System/Library/Fonts"));
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Library/Fonts"));
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_font_file_accepts_font_extensions() {
        assert!(is_font_file(Path::new("foo.ttf")), "must accept .ttf");
        assert!(is_font_file(Path::new("foo.otf")), "must accept .otf");
        assert!(is_font_file(Path::new("foo.ttc")), "must accept .ttc");
        assert!(is_font_file(Path::new("foo.woff2")), "must accept .woff2");
    }

    #[test]
    fn is_font_file_rejects_non_font() {
        assert!(!is_font_file(Path::new("report.typ")), "must reject .typ");
        assert!(!is_font_file(Path::new("data.json")), "must reject .json");
    }

    #[test]
    fn system_font_dirs_contains_usr_share() {
        let dirs = system_font_dirs();
        assert!(
            dirs.contains(&PathBuf::from("/usr/share/fonts")),
            "must include /usr/share/fonts"
        );
    }
}
