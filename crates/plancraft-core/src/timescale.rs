//! Timescale units shared by the Gantt chart, usage views, the timeline and printing.

use crate::dates::prelude::*;

use crate::calendar::shift;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Hours6,
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

fn midnight(d: Date) -> DateTime {
    d.and_time(Time::MIN)
}

impl Unit {
    /// Start of the unit containing `t`.
    pub fn floor(self, t: DateTime) -> DateTime {
        let d = t.date();
        match self {
            Unit::Hours6 => shift(midnight(d), i64::from(t.hour32() / 6 * 6) * 60),
            Unit::Day => midnight(d),
            Unit::Week => midnight(d.checked_sub_days(Days::new(u64::from(d.weekday().num_days_from_monday()))).unwrap_or(d)),
            Unit::Month => midnight(d.with_day(1).unwrap_or(d)),
            Unit::Quarter => midnight(crate::dates::ymd(d.year32(), (d.month0() / 3) * 3 + 1, 1).unwrap_or(d)),
            Unit::Year => midnight(crate::dates::ymd(d.year32(), 1, 1).unwrap_or(d)),
        }
    }

    /// Start of the next unit. Always later than `t` unless `t` is at the end of the date range.
    pub fn next(self, t: DateTime) -> DateTime {
        let add_months = |t: DateTime, m: u32| {
            let d = t.date();
            let total = d.year32() * 12 + d.month0() as i32 + m as i32;
            // Out of chrono's range: still move forward so callers stepping through units terminate.
            crate::dates::ymd(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1)
                .map_or_else(|| shift(t, 31 * 1440 * i64::from(m)), midnight)
        };
        match self {
            Unit::Hours6 => shift(t, 6 * 60),
            Unit::Day => shift(t, 1440),
            Unit::Week => shift(t, 7 * 1440),
            Unit::Month => add_months(t, 1),
            Unit::Quarter => add_months(t, 3),
            Unit::Year => add_months(t, 12),
        }
    }

    /// Label for the unit starting at `t`, shortened to fit `width` (pixels or points).
    /// Dates are `dd-mm`; `names` gives the day and month names in the interface language.
    pub fn label(self, t: DateTime, width: f32, names: &crate::dates::DateNames) -> String {
        let d = t.date();
        let first = |s: &str| s.chars().next().map(String::from).unwrap_or_default();
        let dm_year = |d: Date| format!("{:02}-{:02}-'{:02}", d.day(), d.month(), d.year().rem_euclid(100));
        match self {
            Unit::Hours6 => format!("{:02}", t.hour32()),
            Unit::Day => {
                if width >= 90.0 {
                    format!("{} {} {}", names.weekday(d), d.day(), names.month_short(d))
                } else if width >= 26.0 {
                    d.day().to_string()
                } else {
                    first(names.weekday(d))
                }
            }
            Unit::Week => {
                if width >= 110.0 {
                    format!("{} {}", names.week_of, dm_year(d))
                } else if width >= 60.0 {
                    dm_year(d)
                } else {
                    d.day().to_string()
                }
            }
            Unit::Month => {
                if width >= 90.0 {
                    format!("{} {}", names.month(d), d.year32())
                } else if width >= 40.0 {
                    format!("{} '{:02}", names.month_short(d), d.year().rem_euclid(100))
                } else {
                    first(names.month_short(d))
                }
            }
            Unit::Quarter => {
                let q = d.month0() / 3 + 1;
                if width >= 60.0 { format!("Q{q} {}", d.year32()) } else { format!("Q{q}") }
            }
            Unit::Year => d.year32().to_string(),
        }
    }
}

/// (top tier, bottom tier) for a zoom level in pixels (or points) per day.
pub fn tiers(per_day: f32) -> (Unit, Unit) {
    if per_day >= 72.0 {
        (Unit::Day, Unit::Hours6)
    } else if per_day >= 14.0 {
        (Unit::Week, Unit::Day)
    } else if per_day >= 4.0 {
        (Unit::Month, Unit::Week)
    } else if per_day >= 1.5 {
        (Unit::Quarter, Unit::Month)
    } else {
        (Unit::Year, Unit::Quarter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_step_forward_even_at_the_edge_of_the_date_range() {
        let all = [Unit::Hours6, Unit::Day, Unit::Week, Unit::Month, Unit::Quarter, Unit::Year];
        let t = crate::dates::ymd(2026, 11, 12).and_then(|d| d.and_hms_opt(13, 0, 0)).unwrap();
        for u in all {
            assert!(u.floor(t) <= t);
            assert!(u.next(u.floor(t)) > t, "{u:?}");
        }
        for u in all {
            // Never panics at chrono's limits.
            let _ = u.next(DateTime::MAX);
            let _ = u.floor(DateTime::MIN);
            let _ = u.label(DateTime::MAX, 100.0, &crate::dates::NAMES_EN);
        }
    }

    #[test]
    fn labels_are_dd_mm_and_speak_the_given_language() {
        let t = crate::dates::ymd(2026, 10, 15).map(crate::dates::midnight).unwrap();
        assert_eq!(Unit::Day.label(t, 100.0, &crate::dates::NAMES_NL), "do 15 okt");
        assert_eq!(Unit::Day.label(t, 100.0, &crate::dates::NAMES_EN), "Thu 15 Oct");
        assert_eq!(Unit::Week.label(t, 120.0, &crate::dates::NAMES_NL), "Week van 15-10-'26");
        assert_eq!(Unit::Week.label(t, 80.0, &crate::dates::NAMES_EN), "15-10-'26");
        assert_eq!(Unit::Month.label(t, 100.0, &crate::dates::NAMES_NL), "oktober 2026");
        assert_eq!(Unit::Month.label(t, 50.0, &crate::dates::NAMES_EN), "Oct '26");
    }
}
