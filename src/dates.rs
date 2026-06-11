//! dates — civil-calendar arithmetic, shared by the market tools, the news
//! ingestion pipeline, and the chart axes. (Howard Hinnant's algorithms.)
//! Factored out of the financial code so every consumer calls one library.

/// "YYYY-MM-DD" -> days since the civil epoch (Howard Hinnant's algorithm).
pub fn date_ordinal(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let day: i64 = it.next()?.parse().ok()?;
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// days since the civil epoch -> "YYYY-MM-DD" (the inverse).
pub fn ordinal_date(z: i64) -> String {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Convenience: today's date is not available without a clock source; the
/// market tools anchor on the data's own end date instead. This helper adds
/// `days` (possibly negative) to a YYYY-MM-DD date.
pub fn add_days(date: &str, days: i64) -> Option<String> {
    date_ordinal(date).map(|o| ordinal_date(o + days))
}

/// Whole days from `a` to `b` (positive when b is later).
pub fn days_between(a: &str, b: &str) -> Option<i64> {
    Some(date_ordinal(b)? - date_ordinal(a)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn civil_round_trip_and_arithmetic() {
        for d in ["2026-06-10", "2024-02-29", "1970-01-01", "1999-12-31"] {
            assert_eq!(ordinal_date(date_ordinal(d).unwrap()), d);
        }
        assert_eq!(add_days("2026-06-10", -7).unwrap(), "2026-06-03");
        assert_eq!(add_days("2026-02-28", 1).unwrap(), "2026-03-01");
        assert_eq!(add_days("2024-02-28", 1).unwrap(), "2024-02-29"); // leap
        assert_eq!(days_between("2026-06-03", "2026-06-10").unwrap(), 7);
    }
}
