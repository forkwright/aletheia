//! Minimal cron expression parser that works directly with [`jiff::Timestamp`].
//!
//! WHY: the `cron` crate requires `chrono`, but the project standard is `jiff`.
//! This module replaces the external `cron` crate with a focused parser that
//! covers the expression features actually used in daemon schedules.
//!
//! Supports:
//! - 5-field (`min hour dom month dow`) and 6-field (`sec min hour dom month dow`)
//! - Wildcards (`*`), ranges (`1-5`), lists (`1,3,5`), steps (`*/15`, `1-5/2`)
//! - Day-of-week names (`MON`..`SUN`), month names (`JAN`..`DEC`)

use std::collections::BTreeSet;

use crate::error;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed cron expression.
#[derive(Debug, Clone)]
pub(crate) struct CronExpr {
    seconds: BTreeSet<u8>,
    minutes: BTreeSet<u8>,
    hours: BTreeSet<u8>,
    days_of_month: BTreeSet<u8>,
    months: BTreeSet<u8>,
    days_of_week: BTreeSet<u8>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

impl CronExpr {
    /// Parse a standard cron expression.
    ///
    /// Accepts 5-field (`min hour dom month dow`) or 6-field
    /// (`sec min hour dom month dow`) formats.
    #[expect(
        clippy::indexing_slicing,
        reason = "indices match the just-validated `fields.len()` arm above"
    )]
    #[expect(
        clippy::similar_names,
        reason = "field names mirror cron section names — disambiguating them with longer/shorter names hurts readability"
    )]
    pub(crate) fn parse(expr: &str) -> Result<Self, error::Error> {
        let fields: Vec<&str> = expr.split_whitespace().collect();

        let (sec_str, minute_str, hour_str, dom_str, month_str, dow_str) = match fields.len()
        {
            5 => ("0", fields[0], fields[1], fields[2], fields[3], fields[4]),
            6 => (
                fields[0], fields[1], fields[2], fields[3], fields[4], fields[5],
            ),
            _ => {
                return Err(error::Error::CronParse {
                    expression: expr.to_owned(),
                    reason: format!("expected 5 or 6 fields, got {}", fields.len()),
                    location: snafu::location!(),
                });
            }
        };

        let seconds = parse_field(sec_str, 0, 59, &[])
            .map_err(|e| cron_error(expr, format!("seconds: {e}")))?;
        let minutes = parse_field(minute_str, 0, 59, &[])
            .map_err(|e| cron_error(expr, format!("minutes: {e}")))?;
        let hours = parse_field(hour_str, 0, 23, &[])
            .map_err(|e| cron_error(expr, format!("hours: {e}")))?;
        let days_of_month = parse_field(dom_str, 1, 31, &[])
            .map_err(|e| cron_error(expr, format!("day-of-month: {e}")))?;
        let months = parse_field(month_str, 1, 12, &MONTH_NAMES)
            .map_err(|e| cron_error(expr, format!("month: {e}")))?;
        let days_of_week = parse_field(dow_str, 0, 6, &DOW_NAMES)
            .map_err(|e| cron_error(expr, format!("day-of-week: {e}")))?;

        Ok(Self {
            seconds,
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
        })
    }

    /// Find the next occurrence strictly after `after`.
    ///
    /// Returns `None` only if the expression can never match (should not
    /// happen for well-formed expressions, but we guard against infinite
    /// loops with a year-limit).
    #[expect(
        clippy::as_conversions,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::too_many_lines,
        reason = "month (1-12), day (1-31), hour (0-23), minute (0-59), second (0-59) all fit in i8 and have no negative values; u8→i8 is safe for values ≤ 59. Function length: this is a single cohesive cron field-walking state machine — splitting would scatter the field-rollover logic and obscure the carry semantics"
    )]
    pub(crate) fn next_after(&self, after: jiff::Timestamp) -> Option<jiff::Timestamp> {
        // Work in UTC civil datetime for field-level manipulation.
        let dt = after.to_zoned(jiff::tz::TimeZone::UTC).datetime();

        let mut year = dt.year();
        let mut month = dt.month();
        let mut day = dt.day();
        let mut hour = dt.hour();
        let mut minute = dt.minute();
        let mut second = dt.second();

        // Advance by one second so we are strictly after `after`.
        second += 1;
        if second > 59 {
            second = 0;
            minute += 1;
        }
        if minute > 59 {
            minute = 0;
            hour += 1;
        }
        if hour > 23 {
            hour = 0;
            day += 1;
        }
        // Day/month overflow handled in the main loop below.

        let max_year = year + 4;

        loop {
            if year > max_year {
                return None;
            }

            // --- Month ---
            match next_value(&self.months, month as u8) {
                Some(m) if m == month as u8 => { /* current month is valid */ }
                Some(m) => {
                    month = m as i8;
                    day = 1;
                    hour = 0;
                    minute = 0;
                    second = 0;
                }
                None => {
                    // No valid month left this year — roll to next year.
                    year += 1;
                    month = first_value(&self.months) as i8;
                    day = 1;
                    hour = 0;
                    minute = 0;
                    second = 0;
                    continue;
                }
            }

            // --- Day of month ---
            let dim = days_in_month(year, month);
            // Clamp day-of-month candidates to actual month length.
            if let Some(d) = next_value_max(&self.days_of_month, day as u8, dim) {
                if d != day as u8 {
                    day = d as i8;
                    hour = 0;
                    minute = 0;
                    second = 0;
                }
            } else {
                // No valid day left in this month — advance month.
                month += 1;
                if month > 12 {
                    month = 1;
                    year += 1;
                }
                day = 1;
                hour = 0;
                minute = 0;
                second = 0;
                continue;
            }

            // --- Day of week check ---
            let dow = day_of_week(year, month, day);
            if !self.days_of_week.contains(&dow) {
                // Advance to the next day.
                day += 1;
                hour = 0;
                minute = 0;
                second = 0;
                if day as u8 > dim {
                    month += 1;
                    if month > 12 {
                        month = 1;
                        year += 1;
                    }
                    day = 1;
                }
                continue;
            }

            // --- Hour ---
            match next_value(&self.hours, hour as u8) {
                Some(h) if h == hour as u8 => {}
                Some(h) => {
                    hour = h as i8;
                    minute = 0;
                    second = 0;
                }
                None => {
                    // Advance to the next day.
                    day += 1;
                    hour = 0;
                    minute = 0;
                    second = 0;
                    if day as u8 > dim {
                        month += 1;
                        if month > 12 {
                            month = 1;
                            year += 1;
                        }
                        day = 1;
                    }
                    continue;
                }
            }

            // --- Minute ---
            match next_value(&self.minutes, minute as u8) {
                Some(m) if m == minute as u8 => {}
                Some(m) => {
                    minute = m as i8;
                    second = 0;
                }
                None => {
                    hour += 1;
                    minute = 0;
                    second = 0;
                    if hour > 23 {
                        hour = 0;
                        day += 1;
                        if day as u8 > dim {
                            month += 1;
                            if month > 12 {
                                month = 1;
                                year += 1;
                            }
                            day = 1;
                        }
                    }
                    continue;
                }
            }

            // --- Second ---
            match next_value(&self.seconds, second as u8) {
                Some(s) if s == second as u8 => {}
                Some(s) => {
                    second = s as i8;
                }
                None => {
                    minute += 1;
                    second = 0;
                    if minute > 59 {
                        minute = 0;
                        hour += 1;
                        if hour > 23 {
                            hour = 0;
                            day += 1;
                            if day as u8 > dim {
                                month += 1;
                                if month > 12 {
                                    month = 1;
                                    year += 1;
                                }
                                day = 1;
                            }
                        }
                    }
                    continue;
                }
            }

            // All fields matched — construct result.
            let civil = jiff::civil::DateTime::new(
                year, month, day, hour, minute, second, 0,
            )
            .ok()?;
            let zoned = civil.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
            return Some(zoned.timestamp());
        }
    }
}

// ---------------------------------------------------------------------------
// Error construction helper
// ---------------------------------------------------------------------------

/// Build a `CronParse` error (for use in `.map_err()` paths).
fn cron_error(expression: &str, reason: String) -> error::Error {
    error::Error::CronParse {
        expression: expression.to_owned(),
        reason,
        location: snafu::location!(),
    }
}

// ---------------------------------------------------------------------------
// Field parsing helpers
// ---------------------------------------------------------------------------

/// Name aliases for month fields.
const MONTH_NAMES: [(&str, u8); 12] = [
    ("JAN", 1),
    ("FEB", 2),
    ("MAR", 3),
    ("APR", 4),
    ("MAY", 5),
    ("JUN", 6),
    ("JUL", 7),
    ("AUG", 8),
    ("SEP", 9),
    ("OCT", 10),
    ("NOV", 11),
    ("DEC", 12),
];

/// Day-of-week: 0=Sunday .. 6=Saturday (standard cron convention).
const DOW_NAMES: [(&str, u8); 7] = [
    ("SUN", 0),
    ("MON", 1),
    ("TUE", 2),
    ("WED", 3),
    ("THU", 4),
    ("FRI", 5),
    ("SAT", 6),
];

/// Internal field-parse error (stringified before surfacing to callers).
#[derive(Debug)]
struct FieldError(String);

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Parse a single cron field (e.g. `"*/15"`, `"1-5"`, `"MON-FRI"`) into a set
/// of valid values.
fn parse_field(
    field: &str,
    min: u8,
    max: u8,
    names: &[(&str, u8)],
) -> Result<BTreeSet<u8>, FieldError> {
    let mut result = BTreeSet::new();

    for part in field.split(',') {
        let (range_part, step) = if let Some((r, s)) = part.split_once('/') {
            let step: u8 = s
                .parse()
                .map_err(|_e| FieldError(format!("invalid step value: {s}")))?;
            if step == 0 {
                return Err(FieldError(format!("step must be > 0, got {step}")));
            }
            (r, Some(step))
        } else {
            (part, None)
        };

        let (range_lo, range_hi) = if range_part == "*" {
            (min, max)
        } else if let Some((lo_s, hi_s)) = range_part.split_once('-') {
            let lo = resolve_value(lo_s, names, min, max)?;
            let hi = resolve_value(hi_s, names, min, max)?;
            if lo > hi {
                return Err(FieldError(format!("range start ({lo}) > end ({hi})")));
            }
            (lo, hi)
        } else {
            let v = resolve_value(range_part, names, min, max)?;
            (v, v)
        };

        match step {
            Some(s) => {
                let mut v = range_lo;
                while v <= range_hi {
                    result.insert(v);
                    v = v.saturating_add(s);
                }
            }
            None => {
                for v in range_lo..=range_hi {
                    result.insert(v);
                }
            }
        }
    }

    if result.is_empty() {
        return Err(FieldError("field produced no valid values".to_owned()));
    }
    Ok(result)
}

/// Resolve a single token to a numeric value, checking name aliases.
fn resolve_value(
    token: &str,
    names: &[(&str, u8)],
    min: u8,
    max: u8,
) -> Result<u8, FieldError> {
    // Try name lookup first.
    let upper = token.to_uppercase();
    for &(name, val) in names {
        if upper == name {
            return Ok(val);
        }
    }

    let v: u8 = token
        .parse()
        .map_err(|_e| FieldError(format!("invalid value: {token}")))?;
    if v < min || v > max {
        return Err(FieldError(format!(
            "value {v} out of range [{min}, {max}]"
        )));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Next-value helpers
// ---------------------------------------------------------------------------

/// Find the first value in `set` that is >= `from`.
fn next_value(set: &BTreeSet<u8>, from: u8) -> Option<u8> {
    set.range(from..).next().copied()
}

/// Like [`next_value`] but also enforces an upper bound (for day-of-month
/// clamping to actual month length).
fn next_value_max(set: &BTreeSet<u8>, from: u8, max: u8) -> Option<u8> {
    set.range(from..=max).next().copied()
}

/// Return the smallest value in `set`.
///
/// Returns 0 if `set` is empty (cannot happen for a successfully parsed field).
fn first_value(set: &BTreeSet<u8>) -> u8 {
    set.iter().next().copied().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Calendar helpers
// ---------------------------------------------------------------------------

/// Number of days in a given month (1-12) for a given year.
fn days_in_month(year: i16, month: i8) -> u8 {
    // Use jiff to get the correct answer including leap years.
    // The `try_into().ok()` keeps the conversion safe — no `as` cast needed.
    jiff::civil::Date::new(year, month, 1)
        .ok()
        .and_then(|d| d.days_in_month().try_into().ok())
        .unwrap_or(30)
}

/// Day of week for a date: 0=Sunday, 1=Monday, ..., 6=Saturday.
///
/// Uses Tomohiko Sakamoto's algorithm.
#[expect(
    clippy::cast_sign_loss,
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "intermediate values are small positive integers that fit in u8/i32; month is 1-12 so m-1 is valid index 0-11"
)]
fn day_of_week(year: i16, month: i8, day: i8) -> u8 {
    static T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = i32::from(year);
    let m = month as usize;
    let d = i32::from(day);
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + T[m - 1] + d) % 7) as u8
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    // -- Parsing --

    #[test]
    fn parse_five_field() {
        let expr = CronExpr::parse("*/15 * * * *").unwrap();
        assert!(expr.seconds.contains(&0));
        assert_eq!(expr.seconds.len(), 1, "5-field defaults to second=0");
        assert_eq!(expr.minutes.len(), 4); // 0, 15, 30, 45
    }

    #[test]
    fn parse_six_field() {
        let expr = CronExpr::parse("30 */15 * * * *").unwrap();
        assert!(expr.seconds.contains(&30));
        assert_eq!(expr.minutes.len(), 4);
    }

    #[test]
    fn parse_range() {
        let expr = CronExpr::parse("0 0 9-17 * * *").unwrap();
        assert_eq!(expr.hours.len(), 9); // 9,10,11,12,13,14,15,16,17
        assert!(expr.hours.contains(&9));
        assert!(expr.hours.contains(&17));
    }

    #[test]
    fn parse_list() {
        let expr = CronExpr::parse("0 0,15,30,45 * * * *").unwrap();
        assert_eq!(expr.minutes.len(), 4);
        assert!(expr.minutes.contains(&0));
        assert!(expr.minutes.contains(&45));
    }

    #[test]
    fn parse_step_on_range() {
        let expr = CronExpr::parse("0 0 1-10/3 * * *").unwrap();
        // 1, 4, 7, 10
        assert_eq!(expr.hours, [1, 4, 7, 10].into_iter().collect());
    }

    #[test]
    fn parse_dow_names() {
        let expr = CronExpr::parse("0 0 * * * MON-FRI").unwrap();
        assert_eq!(expr.days_of_week.len(), 5);
        assert!(expr.days_of_week.contains(&1)); // MON
        assert!(expr.days_of_week.contains(&5)); // FRI
        assert!(!expr.days_of_week.contains(&0)); // SUN
    }

    #[test]
    fn parse_month_names() {
        let expr = CronExpr::parse("0 0 * * JAN,JUN *").unwrap();
        assert_eq!(expr.months.len(), 2);
        assert!(expr.months.contains(&1));
        assert!(expr.months.contains(&6));
    }

    #[test]
    fn parse_invalid_field_count() {
        assert!(CronExpr::parse("* * *").is_err());
    }

    #[test]
    fn parse_invalid_value() {
        assert!(CronExpr::parse("0 0 25 * * *").is_err()); // hour 25
    }

    #[test]
    fn parse_invalid_text() {
        assert!(CronExpr::parse("not a cron expression").is_err());
    }

    // -- next_after --

    #[test]
    fn next_after_every_minute() {
        let expr = CronExpr::parse("0 * * * * *").unwrap();
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap(); // 2023-11-14 22:13:20 UTC
        let next = expr.next_after(base).unwrap();
        assert!(next > base);
        // Should be on the next minute boundary.
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.second(), 0);
    }

    #[test]
    fn next_after_every_hour() {
        let expr = CronExpr::parse("0 0 * * * *").unwrap();
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.minute(), 0);
        assert_eq!(dt.second(), 0);
    }

    #[test]
    fn next_after_every_15_minutes() {
        let expr = CronExpr::parse("0 */15 * * * *").unwrap();
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert!(
            [0, 15, 30, 45].contains(&dt.minute()),
            "minute should be a 15-min boundary, got {}",
            dt.minute()
        );
    }

    #[test]
    fn next_after_specific_day() {
        // Every day at 03:00 UTC
        let expr = CronExpr::parse("0 0 3 * * *").unwrap();
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap(); // 22:13 UTC
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.hour(), 3);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn next_after_month_boundary() {
        // Jan 31 23:59:00 — next hourly should be Feb 1 00:00:00
        let expr = CronExpr::parse("0 0 * * * *").unwrap();
        let base = jiff::civil::DateTime::new(2024, 1, 31, 23, 59, 0, 0)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn next_after_year_rollover() {
        // Dec 31 23:59:00 — next hourly should be Jan 1 00:00:00 of next year
        let expr = CronExpr::parse("0 0 * * * *").unwrap();
        let base = jiff::civil::DateTime::new(2024, 12, 31, 23, 59, 0, 0)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
    }

    #[test]
    fn next_after_dow_filter() {
        // Only Mondays at midnight
        let expr = CronExpr::parse("0 0 0 * * MON").unwrap();
        let base = jiff::Timestamp::from_second(1_700_000_000).unwrap();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        // Verify it's a Monday
        let dow = day_of_week(dt.year(), dt.month(), dt.day());
        assert_eq!(dow, 1, "expected Monday (1), got {dow}");
    }

    #[test]
    fn next_after_complex_expr() {
        // Every 15 min, 9am-5pm, Mon-Fri (6-field)
        let expr = CronExpr::parse("0 */15 9-17 * * MON-FRI").unwrap();
        let next = expr.next_after(jiff::Timestamp::now());
        assert!(next.is_some(), "complex expr should produce a result");
    }

    #[test]
    fn next_after_feb_29_leap_year() {
        // 2024 is a leap year. Schedule on the 29th of every month.
        let expr = CronExpr::parse("0 0 0 29 * *").unwrap();
        let base = jiff::civil::DateTime::new(2024, 2, 28, 12, 0, 0, 0)
            .unwrap()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp();
        let next = expr.next_after(base).unwrap();
        let dt = next.to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 29);
    }

    #[test]
    fn next_after_five_field() {
        // 5-field: every 30 minutes
        let expr = CronExpr::parse("*/30 * * * *").unwrap();
        let next = expr.next_after(jiff::Timestamp::now());
        assert!(next.is_some());
        let dt = next.unwrap().to_zoned(jiff::tz::TimeZone::UTC).datetime();
        assert_eq!(dt.second(), 0);
        assert!(dt.minute() == 0 || dt.minute() == 30);
    }
}
