use super::*;

#[test]
fn days_cover_years_and_months() {
    assert_eq!(parse_day("2026", false).unwrap(), NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
    assert_eq!(parse_day("2026", true).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    assert_eq!(parse_day("2026-02", true).unwrap(), NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    assert_eq!(parse_day("2026-09-30", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 30).unwrap());
    assert!(parse_day("last week", false).is_err());
    assert!(parse_day("2026-13", false).is_err());
}

#[test]
fn a_month_starts_on_its_first_and_a_day_has_three_parts_at_most() {
    assert_eq!(parse_day("2026-09", false).unwrap(), NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
    assert!(parse_day("2026-09-30-12", false).is_err());
    assert!(parse_day("1900", false).is_err(), "too long ago to be a set");
}
