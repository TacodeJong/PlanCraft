//! A calendar popup. Dates are shown day-month-year. Choosing a day returns a civil
//! [`DateTime`] at the scheduler's start or finish minute. The plan stores that value;
//! the button never parses what it displays.

use egui::{Id, RichText, Stroke, Ui, vec2};
use plancraft_core::dates::prelude::*;

use crate::theme;

/// What the button shows and when a chosen day is placed.
pub struct DateField<'a> {
    pub salt: &'a str,
    pub current: Option<DateTime>,
    pub clearable: bool,
    pub enabled: bool,
    /// Minutes from midnight. A start uses the plan's default start time, a finish its finish time.
    pub minute: u32,
    pub empty: &'a str,
}

/// `Some(value)` when the user picks a day or clears the field. `value` is `None` when cleared.
pub fn button(ui: &mut Ui, field: DateField<'_>) -> Option<Option<DateTime>> {
    let label = field.current.map(format_dutch).unwrap_or_else(|| field.empty.to_string());
    let resp = ui.add_enabled(field.enabled, egui::Button::new(label).min_size(vec2(148.0, 0.0)));
    let mut change = None;
    egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if let Some(next) = calendar(ui, field.salt, field.current, field.clearable, field.minute) {
            change = Some(next);
            ui.close();
        }
    });
    change
}

/// `15-10-2026 08:00`.
pub fn format_dutch(dt: DateTime) -> String {
    format!("{:02}-{:02}-{:04} {:02}:{:02}", dt.day(), dt.month(), dt.year(), dt.hour(), dt.minute())
}

/// `15-10-2026`.
pub fn format_day(dt: DateTime) -> String {
    format!("{:02}-{:02}-{:04}", dt.day(), dt.month(), dt.year())
}

fn calendar(ui: &mut Ui, salt: &str, current: Option<DateTime>, clearable: bool, minute: u32) -> Option<Option<DateTime>> {
    let id = Id::new(("date_month", salt));
    let today = plancraft_core::dates::today();
    let (mut year, mut month) = ui
        .ctx()
        .data(|d| d.get_temp::<(i32, i8)>(id))
        .unwrap_or_else(|| current.map(|dt| (i32::from(dt.year()), dt.month())).unwrap_or((i32::from(today.year()), today.month())));
    let mut picked = None;
    ui.set_min_width(230.0);
    ui.horizontal(|ui| {
        if ui.button("‹").clicked() {
            (year, month) = shift_month(year, month, -1);
        }
        ui.label(RichText::new(format!("{} {year}", month_name(month))).strong().color(theme::pal().TEXT));
        if ui.button("›").clicked() {
            (year, month) = shift_month(year, month, 1);
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, (year, month)));
    ui.horizontal(|ui| {
        for col in 0..7 {
            ui.add_sized(vec2(28.0, 16.0), egui::Label::new(RichText::new(weekday_name(col)).size(11.0).color(theme::pal().TEXT_WEAK)));
        }
    });
    let first = ymd_ok(year, month, 1)?;
    let selected = current.map(|dt| dt.date());
    for row in 0..6 {
        ui.horizontal(|ui| {
            for col in 0..7 {
                let offset = row * 7 + col - monday_index(first);
                let Some(day) = plancraft_core::dates::add_days(first, i64::from(offset)) else { continue };
                let other = day.month() != month;
                let weekend = col >= 5;
                let is_sel = selected == Some(day);
                let is_today = day == today;
                let mut text = RichText::new(day.day().to_string());
                if is_sel {
                    text = text.color(theme::pal().CHIP_TEXT);
                } else if other || weekend {
                    text = text.color(theme::pal().TEXT_WEAK);
                }
                let mut btn = egui::Button::new(text).min_size(vec2(28.0, 24.0));
                if is_sel {
                    btn = btn.fill(theme::pal().ACCENT);
                }
                if is_today {
                    btn = btn.stroke(Stroke::new(1.0, theme::pal().ACCENT));
                }
                if ui.add(btn).clicked()
                    && let Some(dt) = at_minute(day, minute)
                {
                    picked = Some(Some(dt));
                }
            }
        });
    }
    if clearable && ui.button("geen").clicked() {
        picked = Some(None);
    }
    picked
}

fn at_minute(day: Date, minute: u32) -> Option<DateTime> {
    let hour = minute / 60;
    let min = minute % 60;
    plancraft_core::dates::ymd_hm(i32::from(day.year()), u32::try_from(day.month()).ok()?, u32::try_from(day.day()).ok()?, hour, min)
}

fn ymd_ok(year: i32, month: i8, day: i8) -> Option<Date> {
    plancraft_core::dates::ymd(year, u32::try_from(month).ok()?, u32::try_from(day).ok()?)
}

fn shift_month(year: i32, month: i8, delta: i32) -> (i32, i8) {
    let m = i32::from(month) + delta;
    let year = year + (m - 1).div_euclid(12);
    let month = (m - 1).rem_euclid(12) + 1;
    (year, i8::try_from(month).unwrap_or(1))
}

fn monday_index(day: Date) -> i32 {
    match day.weekday() {
        Weekday::Monday => 0,
        Weekday::Tuesday => 1,
        Weekday::Wednesday => 2,
        Weekday::Thursday => 3,
        Weekday::Friday => 4,
        Weekday::Saturday => 5,
        Weekday::Sunday => 6,
    }
}

fn weekday_name(col: i32) -> &'static str {
    match col {
        0 => "ma",
        1 => "di",
        2 => "wo",
        3 => "do",
        4 => "vr",
        5 => "za",
        _ => "zo",
    }
}

fn month_name(month: i8) -> &'static str {
    match month {
        1 => "januari",
        2 => "februari",
        3 => "maart",
        4 => "april",
        5 => "mei",
        6 => "juni",
        7 => "juli",
        8 => "augustus",
        9 => "september",
        10 => "oktober",
        11 => "november",
        _ => "december",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dutch_display_is_day_month_year() {
        let start = plancraft_core::dates::ymd_hm(2026, 10, 15, 8, 0).unwrap();
        let finish = plancraft_core::dates::ymd_hm(2026, 11, 30, 17, 0).unwrap();
        assert_eq!(format_dutch(start), "15-10-2026 08:00");
        assert_eq!(format_dutch(finish), "30-11-2026 17:00");
        assert_eq!(format_day(start), "15-10-2026");
    }
}
