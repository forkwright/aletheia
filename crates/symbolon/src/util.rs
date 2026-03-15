//! Internal utilities shared across `symbolon` modules.

/// Convert days since the Unix epoch (1970-01-01) to a `(year, month, day)` triple.
///
/// Implements the civil-date algorithm by Howard Hinnant.
pub(crate) fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let y = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(days_to_date(0), (1970, 1, 1));
    }

    #[test]
    fn known_date_2023_11_14() {
        // 2023-11-14 = 19_675 days since epoch
        assert_eq!(days_to_date(19_675), (2023, 11, 14));
    }
}
