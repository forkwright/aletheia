//! Shared filesystem and date fixtures for maintenance-task tests.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use jiff::Timestamp;
use jiff::civil::Date;

/// Write `content` to `path` and set its permissions to `0o644`.
pub(crate) fn write_fixture(path: impl AsRef<Path>, content: &str) {
    #[expect(
        clippy::disallowed_methods,
        reason = "test fixture: synchronous write in non-async test context"
    )]
    fs::write(path.as_ref(), content).expect("write fixture");
    let mut perms = fs::metadata(path.as_ref())
        .expect("read fixture metadata")
        .permissions();
    perms.set_mode(0o644);
    fs::set_permissions(path.as_ref(), perms).expect("set fixture permissions");
}

/// Today's date in UTC.
pub(crate) fn utc_today() -> Date {
    Timestamp::now().to_zoned(jiff::tz::TimeZone::UTC).date()
}
