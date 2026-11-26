//! Display formatting helpers.
//!
//! Every date on screen is `dd-mm-yyyy` (`15-10-2026`), in both interface languages, matching
//! the date pickers and the properties pane. Only day and month *names* differ per language
//! ([`names`]): "do 15 okt" in Dutch, "Thu 15 Oct" in English.

use plancraft_core::dates::prelude::*;
use plancraft_core::dates::{self, DateNames};
use plancraft_core::prelude::*;

/// The day and month names of the interface language on screen.
pub fn names() -> &'static DateNames {
    match crate::lang::current() {
        crate::lang::Lang::En => &dates::NAMES_EN,
        crate::lang::Lang::Nl => &dates::NAMES_NL,
    }
}

/// `15-10-2026`, or `NA`: the one way a date is shown (table columns, cards, tooltips, reports).
pub fn date(dt: Option<DateTime>) -> String {
    dt.map(|d| dates::dmy(d.date())).unwrap_or_else(|| "NA".into())
}

/// `15-10-2026 15:30`, or `NA`: a date with the clock the user set.
pub fn date_hm(dt: Option<DateTime>) -> String {
    dt.map(|d| format!("{} {:02}:{:02}", dates::dmy(d.date()), d.hour32(), d.minute32())).unwrap_or_else(|| "NA".into())
}

/// `do 15-10-2026 08:00`, or `NA`: a date with its weekday and time, where there is room.
pub fn date_long(dt: Option<DateTime>) -> String {
    dt.map(|d| format!("{} {} {:02}:{:02}", names().weekday(d.date()), dates::dmy(d.date()), d.hour32(), d.minute32()))
        .unwrap_or_else(|| "NA".into())
}

/// `15-10-2026` for a plain date.
pub fn day(d: Date) -> String {
    dates::dmy(d)
}

/// `do 15-10`: a weekday and short date for narrow columns (usage and timesheet headers).
pub fn weekday_day(d: Date) -> String {
    format!("{} {:02}-{:02}", names().weekday(d), d.day32(), d.month32())
}

/// `do 15-10-2026`: a full date with its weekday (the timeline's ends).
pub fn weekday_date(d: Date) -> String {
    format!("{} {}", names().weekday(d), dates::dmy(d))
}

/// `15-10` for tight chart labels.
pub fn day_month(d: Date) -> String {
    format!("{:02}-{:02}", d.day32(), d.month32())
}

/// `1 okt`: the day with its month name (calendar cells on the first of a month).
pub fn day_month_name(d: Date) -> String {
    format!("{} {}", d.day32(), names().month_short(d))
}

/// `oktober 2026`: a month header in the interface language.
pub fn month_year(d: Date) -> String {
    format!("{} {}", names().month(d), d.year32())
}

/// `okt 2026`: a short month and year.
pub fn month_short_year(d: Date) -> String {
    format!("{} {}", names().month_short(d), d.year32())
}

pub fn iso(dt: Option<DateTime>) -> String {
    dt.map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

pub fn iso_date(dt: Option<DateTime>) -> String {
    dt.map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

pub fn money(p: &Project, v: f64) -> String {
    let neg = v < 0.0;
    let cents = (v.abs() * 100.0).round() as i64;
    let whole = cents / 100;
    let frac = cents % 100;
    let mut s = whole.to_string();
    let mut i = s.len() as isize - 3;
    while i > 0 {
        s.insert(i as usize, ',');
        i -= 3;
    }
    format!("{}{}{s}.{frac:02}", if neg { "-" } else { "" }, p.currency())
}

pub fn hours(mins: f64) -> String {
    let h = mins / 60.0;
    if (h - h.round()).abs() < 0.05 { format!("{}h", h.round() as i64) } else { format!("{h:.1}h") }
}

pub fn percent(v: f32) -> String {
    format!("{}%", v.round() as i64)
}

pub fn units(u: f64) -> String {
    format!("{}%", (u * 100.0).round() as i64)
}

/// Parses "50%", "50", "0.5" into units (fraction).
pub fn parse_units(text: &str) -> Option<f64> {
    let t = text.trim();
    let units = if let Some(p) = t.strip_suffix('%') {
        p.trim().parse::<f64>().ok()? / 100.0
    } else {
        let v: f64 = t.parse().ok()?;
        // Bare numbers are percentages ("50" = 50%) unless written as a small fraction ("0.5", "1.5").
        if t.contains('.') && v <= 10.0 { v } else { v / 100.0 }
    };
    // "NaN", "inf" and negative values parse as floats but are not units.
    (units.is_finite() && units >= 0.0).then_some(units)
}

/// Parses money like "$1,200.50".
pub fn parse_money(text: &str) -> Option<f64> {
    let cleaned: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    cleaned.parse().ok()
}

pub fn slack(p: &Project, m: Minutes) -> String {
    p.format_duration(m, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one date format: `dd-mm-yyyy` everywhere, day and month names per language.
    #[test]
    fn dates_are_dd_mm_yyyy_in_both_languages() {
        let dt = plancraft_core::dates::ymd_hm(2026, 10, 15, 8, 0);
        let d = plancraft_core::dates::ymd(2026, 10, 15).unwrap();
        // Dutch is the default and other tests' app frames keep it there; setting it again
        // avoids an order dependency without racing them.
        crate::lang::set_current(crate::lang::Lang::Nl);
        assert_eq!(date(dt), "15-10-2026");
        assert_eq!(date(None), "NA");
        assert_eq!(date_long(dt), "do 15-10-2026 08:00");
        assert_eq!(day(d), "15-10-2026");
        assert_eq!(weekday_day(d), "do 15-10");
        assert_eq!(weekday_date(d), "do 15-10-2026");
        assert_eq!(day_month(d), "15-10");
        assert_eq!(day_month_name(d), "15 okt");
        assert_eq!(month_year(d), "oktober 2026");
        assert_eq!(month_short_year(d), "okt 2026");
        // English names, read from the table so the global language is left alone.
        use plancraft_core::dates::{NAMES_EN, dmy};
        assert_eq!(dmy(d), "15-10-2026", "the order does not change with the language");
        assert_eq!(NAMES_EN.weekday(d), "Thu");
        assert_eq!(NAMES_EN.month(d), "October");
        assert_eq!(NAMES_EN.month_short(d), "Oct");
    }

    #[test]
    fn units_and_money() {
        assert_eq!(parse_units("50%"), Some(0.5));
        assert_eq!(parse_units("150"), Some(1.5));
        assert_eq!(parse_units("0.25"), Some(0.25));
        for bad in ["abc", "", "NaN", "inf", "-50%", "-1"] {
            assert_eq!(parse_units(bad), None, "{bad}");
        }
        assert_eq!(parse_money("$1,200.50"), Some(1200.5));
        let p = Project::new("x", plancraft_core::dates::ymd(2026, 1, 5).unwrap());
        assert_eq!(money(&p, 1234567.891), "$1,234,567.89");
        assert_eq!(money(&p, -5.0), "-$5.00");
    }
}
