//! A calendar popup. Dates are shown day-month-year. Choosing a day returns a civil
//! [`DateTime`] at the scheduler's start or finish minute, unless the field asks for a clock:
//! then the time box starts empty and a day is not stored until the user types a time.
//! The plan stores that value; the button never parses what it displays.

use egui::{Id, Key, RichText, Stroke, Ui, vec2};
use plancraft_core::dates::prelude::*;

use crate::theme;

/// What the button shows and when a chosen day is placed.
pub struct DateField<'a> {
    pub salt: &'a str,
    pub current: Option<DateTime>,
    pub clearable: bool,
    pub enabled: bool,
    /// Minutes from midnight. A start uses the plan's default start time, a finish its finish time.
    /// Ignored when [`Self::choose_time`] is set: that clock comes only from the time box.
    pub minute: u32,
    pub empty: &'a str,
    /// The popup asks for a clock. A day click does not commit, and no 08:00 or 17:00 is filled in.
    pub choose_time: bool,
    /// Draw `current`'s clock in the button and the time box. When false, the button shows the day
    /// alone and the time box is empty, even if `current` carries a calculated time such as 17:00.
    pub show_time: bool,
}

/// `Some(value)` when the user picks a day or clears the field. `value` is `None` when cleared.
pub fn button(ui: &mut Ui, field: DateField<'_>) -> Option<Option<DateTime>> {
    let label = match field.current {
        Some(dt) if !field.choose_time || field.show_time => format_dutch(dt),
        Some(dt) => format_day(dt),
        None => field.empty.to_string(),
    };
    let mut change = None;
    ui.push_id(field.salt, |ui| {
        let clock = ui.scope_id().with("clock");
        let resp = ui.add_enabled(field.enabled, egui::Button::new(label).min_size(vec2(168.0, 0.0)));
        let open =
            egui::Popup::from_toggle_button_response(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                if let Some(next) =
                    calendar(ui, field.salt, field.current, field.clearable, field.minute, field.choose_time, field.show_time, clock)
                {
                    change = Some(next);
                    ui.close();
                }
            });
        // A closed popup forgets a half-typed time, so the next open starts empty again
        // unless a clock was actually stored on the milestone.
        if open.is_none() {
            ui.ctx().data_mut(|d| {
                d.remove_temp::<String>(clock.with("day"));
                d.remove_temp::<String>(clock.with("hm"));
            });
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

/// One day cell's fixed size: every cell identical, a clean 7-column grid.
const CELL: f32 = 30.0;
/// Space between cells.
const GAP: f32 = 2.0;
/// The grid's full width: seven cells and the six gaps between them.
const GRID_W: f32 = 7.0 * CELL + 6.0 * GAP;

/// Month grid used by [`button`] and by the task-sheet date cell editor.
///
/// `choose_time` adds a time box that starts empty unless `show_time` is set. A day click then
/// only highlights the day; the popup commits when the user sets a clock (or clears the field).
pub(crate) fn calendar(
    ui: &mut Ui,
    salt: &str,
    current: Option<DateTime>,
    clearable: bool,
    minute: u32,
    choose_time: bool,
    show_time: bool,
    clock: Id,
) -> Option<Option<DateTime>> {
    let id = Id::unique(("date_month", salt));
    let today = plancraft_core::dates::today();
    let (mut year, mut month) = ui
        .ctx()
        .data(|d| d.get_temp::<(i32, i8)>(id))
        .unwrap_or_else(|| current.map(|dt| (i32::from(dt.year()), dt.month())).unwrap_or((i32::from(today.year()), today.month())));
    let mut picked = None;
    ui.set_min_width(GRID_W);
    ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP);
    // Header: the month arrows at the far edges, the month and year centered between them.
    ui.horizontal(|ui| {
        if crate::icons::button(ui, "chevron-left", 24.0, crate::lang::text("cal.prev_month")).clicked() {
            (year, month) = shift_month(year, month, -1);
        }
        let label = RichText::new(format!("{} {year}", month_name(month))).strong().color(theme::pal().TEXT);
        ui.add_sized(vec2(GRID_W - 2.0 * (24.0 + GAP), 24.0), egui::Label::new(label));
        if crate::icons::button(ui, "chevron-right", 24.0, crate::lang::text("cal.next_month")).clicked() {
            (year, month) = shift_month(year, month, 1);
        }
    });
    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top() + 1.0, Stroke::new(1.0, theme::pal().LINE));
    ui.add_space(3.0);
    ui.ctx().data_mut(|d| d.insert_temp(id, (year, month)));
    ui.horizontal(|ui| {
        for col in 0..7 {
            ui.add_sized(vec2(CELL, 16.0), egui::Label::new(RichText::new(weekday_name(col)).size(11.0).color(theme::pal().TEXT_WEAK)));
        }
    });
    let day_id = clock.with("day");
    let hm_id = clock.with("hm");
    let mut picked_day =
        ui.ctx().data(|d| d.get_temp::<String>(day_id)).and_then(|k| parse_day_key(&k)).or_else(|| current.map(|dt| dt.date()));
    let mut hm = ui.ctx().data(|d| d.get_temp::<String>(hm_id)).unwrap_or_else(|| {
        if show_time { current.map(|dt| format!("{:02}:{:02}", dt.hour32(), dt.minute32())).unwrap_or_default() } else { String::new() }
    });
    let first = ymd_ok(year, month, 1)?;
    let selected = if choose_time { picked_day } else { current.map(|dt| dt.date()) };
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
                    // Adjacent-month and weekend days stay dimmed but keep the same cell size.
                    text = text.color(theme::pal().TEXT_WEAK);
                }
                let mut btn = egui::Button::new(text).corner_radius(3.0);
                if is_sel {
                    btn = btn.fill(theme::pal().ACCENT);
                }
                if is_today {
                    btn = btn.stroke(Stroke::new(1.0, theme::pal().ACCENT));
                }
                if ui.add_sized(vec2(CELL, 24.0), btn).clicked() {
                    if choose_time {
                        // The day is not a time. 17:00 is not filled in; the time box below is.
                        picked_day = Some(day);
                    } else if let Some(dt) = at_minute(day, minute) {
                        picked = Some(Some(dt));
                    }
                }
            }
        });
    }
    if choose_time {
        ui.ctx().data_mut(|d| {
            if let Some(day) = picked_day {
                d.insert_temp(day_id, day_key(day));
            }
            d.insert_temp(hm_id, hm.clone());
        });
        ui.add_space(4.0);
        ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, theme::pal().LINE));
        ui.add_space(4.0);
        let mut commit_time = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::lang::text("cal.time")).size(12.0).color(theme::pal().TEXT));
            let edit = ui.add(egui::TextEdit::singleline(&mut hm).hint_text(crate::lang::text("cal.time_hint")).desired_width(72.0));
            let ready = parse_hm(&hm).is_some() && picked_day.is_some();
            if ui.add_enabled(ready, egui::Button::new(crate::lang::text("cal.set_time"))).clicked()
                || (edit.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
            {
                commit_time = true;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(hm_id, hm.clone()));
        if commit_time
            && let (Some(day), Some((hour, min))) = (picked_day, parse_hm(&hm))
            && let Some(dt) = at_hm(day, hour, min)
        {
            picked = Some(Some(dt));
        }
    }
    if clearable {
        ui.add_space(2.0);
        if ui.add_sized(vec2(GRID_W, 24.0), egui::Button::new("geen")).clicked() {
            picked = Some(None);
        }
    }
    picked
}

/// `15:30`, `15.30`, `1530` or `930`. Empty text is not a time: nothing is invented.
pub(crate) fn parse_hm(text: &str) -> Option<(u32, u32)> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some((hour, min)) = t.split_once(':').or_else(|| t.split_once('.')) {
        return hm_parts(hour, min);
    }
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    match digits.len() {
        3 => {
            let hour = digits.chars().next().and_then(|c| c.to_digit(10))?;
            let min: String = digits.chars().skip(1).collect();
            hm_nums(hour, min.parse().ok()?)
        }
        4 => {
            let hour: u32 = digits.chars().take(2).collect::<String>().parse().ok()?;
            let min: u32 = digits.chars().skip(2).collect::<String>().parse().ok()?;
            hm_nums(hour, min)
        }
        _ => None,
    }
}

fn hm_parts(hour: &str, min: &str) -> Option<(u32, u32)> {
    let hour = hour.trim().parse().ok()?;
    let min: String = min.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    hm_nums(hour, min.parse().ok()?)
}

fn hm_nums(hour: u32, min: u32) -> Option<(u32, u32)> {
    if hour > 23 || min > 59 { None } else { Some((hour, min)) }
}

fn at_hm(day: Date, hour: u32, min: u32) -> Option<DateTime> {
    plancraft_core::dates::ymd_hm(i32::from(day.year()), u32::try_from(day.month()).ok()?, u32::try_from(day.day()).ok()?, hour, min)
}

fn day_key(day: Date) -> String {
    format!("{:04}-{:02}-{:02}", i32::from(day.year()), day.month32(), day.day32())
}

fn parse_day_key(text: &str) -> Option<Date> {
    let (year, rest) = text.split_once('-')?;
    let (month, day) = rest.split_once('-')?;
    plancraft_core::dates::ymd(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
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
    let en = crate::lang::current() == crate::lang::Lang::En;
    match col {
        0 => {
            if en {
                "mo"
            } else {
                "ma"
            }
        }
        1 => {
            if en {
                "tu"
            } else {
                "di"
            }
        }
        2 => {
            if en {
                "we"
            } else {
                "wo"
            }
        }
        3 => {
            if en {
                "th"
            } else {
                "do"
            }
        }
        4 => {
            if en {
                "fr"
            } else {
                "vr"
            }
        }
        5 => {
            if en {
                "sa"
            } else {
                "za"
            }
        }
        _ => {
            if en {
                "su"
            } else {
                "zo"
            }
        }
    }
}

fn month_name(month: i8) -> &'static str {
    let en = crate::lang::current() == crate::lang::Lang::En;
    match month {
        1 => {
            if en {
                "January"
            } else {
                "januari"
            }
        }
        2 => {
            if en {
                "February"
            } else {
                "februari"
            }
        }
        3 => {
            if en {
                "March"
            } else {
                "maart"
            }
        }
        4 => {
            if en {
                "April"
            } else {
                "april"
            }
        }
        5 => {
            if en {
                "May"
            } else {
                "mei"
            }
        }
        6 => {
            if en {
                "June"
            } else {
                "juni"
            }
        }
        7 => {
            if en {
                "July"
            } else {
                "juli"
            }
        }
        8 => {
            if en {
                "August"
            } else {
                "augustus"
            }
        }
        9 => "september",
        10 => {
            if en {
                "October"
            } else {
                "oktober"
            }
        }
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

    #[test]
    fn a_blank_time_is_not_a_clock() {
        assert!(parse_hm("").is_none());
        assert!(parse_hm("   ").is_none());
        assert_eq!(parse_hm("15:30"), Some((15, 30)));
        assert_eq!(parse_hm("15.30"), Some((15, 30)));
        assert_eq!(parse_hm("9:05"), Some((9, 5)));
        assert_eq!(parse_hm("1530"), Some((15, 30)));
        assert_eq!(parse_hm("930"), Some((9, 30)));
        assert!(parse_hm("25:00").is_none());
    }
}
