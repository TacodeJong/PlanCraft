//! Project Information and Options dialog.

use egui::{Context, RichText};
use plancraft_core::dates::prelude::*;
use plancraft_core::prelude::*;
use plancraft_core::workspace::Workspace;

use crate::format;
use crate::theme;

#[derive(Clone)]
pub struct ProjectInfo {
    /// The fields as the dialog opened: OK writes only what was changed since, so changes an
    /// agent made meanwhile are kept.
    orig: Option<Box<ProjectInfo>>,
    name: String,
    manager: String,
    company: String,
    author: String,
    notes: String,
    start: String,
    status_date: String,
    calendar: CalendarId,
    currency: String,
    hours_per_day: String,
    hours_per_week: String,
    days_per_month: String,
    hours_per_point: String,
    unit: DurationUnit,
    new_auto: bool,
    use_res_cal: bool,
    progress_from_time: bool,
    rules_on: bool,
    critical_slack: String,
    ev_baseline: u8,
    error: Option<String>,
}

impl ProjectInfo {
    pub fn new(p: &Project) -> Self {
        let s = &p.settings;
        let mut info = Self {
            orig: None,
            name: p.name.clone(),
            manager: p.manager.clone(),
            company: p.company.clone(),
            author: p.author.clone(),
            notes: p.notes.clone(),
            start: format::iso(Some(p.start)),
            status_date: p.status_date.map(|d| d.to_string()).unwrap_or_default(),
            calendar: p.calendar,
            currency: s.currency.clone(),
            hours_per_day: format!("{}", s.time.minutes_per_day as f64 / 60.0),
            hours_per_week: format!("{}", s.time.minutes_per_week as f64 / 60.0),
            days_per_month: s.time.days_per_month.to_string(),
            hours_per_point: format!("{}", s.minutes_per_point as f64 / 60.0),
            unit: s.duration_unit,
            new_auto: s.new_tasks_auto,
            use_res_cal: s.use_resource_calendars,
            progress_from_time: s.progress_from_time,
            rules_on: s.rules.enabled(),
            critical_slack: p.format_duration(s.critical_slack, false),
            ev_baseline: s.earned_value_baseline,
            error: None,
        };
        info.orig = Some(Box::new(info.clone()));
        info
    }

    /// Writes the fields changed since the dialog opened, in one edit.
    fn apply(&self, p: &mut Project) -> Result<(), String> {
        let Some(o) = self.orig.as_deref() else { return Ok(()) };
        let num = |s: &str, what: &str| s.trim().parse::<f64>().map_err(|_| format!("{what} must be a number"));
        if self.name != o.name {
            p.name = self.name.trim().to_string();
        }
        if self.manager != o.manager {
            p.manager = self.manager.clone();
        }
        if self.company != o.company {
            p.company = self.company.clone();
        }
        if self.author != o.author {
            p.author = self.author.clone();
        }
        if self.notes != o.notes {
            p.notes = self.notes.clone();
        }
        if self.start != o.start {
            p.start = p.parse_datetime(&self.start, false).ok_or("Start date must look like YYYY-MM-DD")?;
        }
        if self.status_date != o.status_date {
            // "NA" (or nothing) means no status date: progress is reported as of today.
            p.status_date = if self.status_date.trim().is_empty() || self.status_date.trim().eq_ignore_ascii_case("na") {
                None
            } else {
                Some(Date::parse_from_str(self.status_date.trim(), "%Y-%m-%d").map_err(|_| "Status date must look like YYYY-MM-DD")?)
            };
        }
        if self.calendar != o.calendar {
            p.calendar = self.calendar;
        }
        let s = &mut p.settings;
        if self.currency != o.currency {
            s.currency = self.currency.clone();
        }
        if self.hours_per_day != o.hours_per_day {
            s.time.minutes_per_day = (num(&self.hours_per_day, "Hours per day")? * 60.0).round() as i64;
            if s.time.minutes_per_day <= 0 {
                return Err("Hours per day must be positive".into());
            }
        }
        if self.hours_per_week != o.hours_per_week {
            s.time.minutes_per_week = (num(&self.hours_per_week, "Hours per week")? * 60.0).round() as i64;
        }
        if self.days_per_month != o.days_per_month {
            s.time.days_per_month = num(&self.days_per_month, "Days per month")?.round() as i64;
        }
        if self.hours_per_point != o.hours_per_point {
            s.minutes_per_point = (num(&self.hours_per_point, "Hours per point")? * 60.0).round() as i64;
        }
        if self.unit != o.unit {
            s.duration_unit = self.unit;
        }
        if self.new_auto != o.new_auto {
            s.new_tasks_auto = self.new_auto;
        }
        if self.use_res_cal != o.use_res_cal {
            s.use_resource_calendars = self.use_res_cal;
        }
        if self.progress_from_time != o.progress_from_time {
            s.progress_from_time = self.progress_from_time;
        }
        if self.rules_on != o.rules_on {
            s.rules.stretch = self.rules_on;
            s.rules.span = self.rules_on;
        }
        // Re-read with the new hours per day (or week, month), as typed: "1 day" stays one day.
        let time_changed =
            self.hours_per_day != o.hours_per_day || self.hours_per_week != o.hours_per_week || self.days_per_month != o.days_per_month;
        if self.critical_slack != o.critical_slack || time_changed {
            let ts = s.time.clone();
            s.critical_slack =
                parse_duration(&self.critical_slack, DurationUnit::Days, &ts).map(|(m, _, _)| m).ok_or("Invalid critical slack")?;
        }
        if self.ev_baseline != o.ev_baseline {
            p.set_earned_value_baseline(self.ev_baseline).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn show(&mut self, ctx: &Context, ws: &mut Workspace, handle: u64) -> bool {
        let mut open = true;
        let mut close = false;
        let mut apply = false;
        {
            let Some(doc) = ws.index_of(handle).and_then(|i| ws.docs.get_mut(i)) else {
                return false;
            };
            let title = format!("{} '{}'", crate::lang::text("dlg.project_info"), doc.project.name);
            egui::Window::new(&title)
                .id(egui::Id::unique("project_info"))
                .pivot(egui::Align2::CENTER_TOP)
                .default_pos(ctx.content_rect().center_top() + egui::vec2(0.0, 120.0))
                .title_bar(false)
                .default_width(600.0)
                .show(ctx, |ui| {
                    open = crate::widgets::craft::dialog_header(ui, &title);
                    egui::Grid::new("pi").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Project name:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.name));
                        ui.label("Calendar:");
                        let cal_name = doc.project.calendar(self.calendar).map(|c| c.name.clone()).unwrap_or_default();
                        egui::ComboBox::from_id_salt("pi_cal").selected_text(cal_name).show_ui(ui, |ui| {
                            for c in &doc.project.calendars {
                                ui.selectable_value(&mut self.calendar, c.id, &c.name);
                            }
                        });
                        ui.end_row();
                        ui.label("Start date:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.start));
                        ui.label("Status date:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.status_date).hint_text("NA (today)"));
                        ui.end_row();
                        ui.label("Finish date:");
                        ui.label(RichText::new(format::date_long(doc.project.sched.finish)).color(theme::pal().TEXT_WEAK));
                        ui.label("Manager:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.manager));
                        ui.end_row();
                        ui.label("Company:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.company));
                        ui.label("Author:");
                        ui.add(crate::widgets::craft::text_edit(&mut self.author));
                        ui.end_row();
                    });
                    ui.label("Notes:");
                    ui.add(crate::widgets::craft::text_edit_multiline(&mut self.notes).desired_rows(3).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    egui::CollapsingHeader::new(RichText::new("Options: schedule, calendar & estimates").strong()).default_open(true).show(
                        ui,
                        |ui| {
                            egui::Grid::new("pi_opts").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
                                ui.label("Hours per day:");
                                ui.add(crate::widgets::craft::text_edit(&mut self.hours_per_day).desired_width(60.0));
                                ui.label("Hours per week:");
                                ui.add(crate::widgets::craft::text_edit(&mut self.hours_per_week).desired_width(60.0));
                                ui.end_row();
                                ui.label("Days per month:");
                                ui.add(crate::widgets::craft::text_edit(&mut self.days_per_month).desired_width(60.0));
                                ui.label("Hours per complexity point:");
                                ui.add(crate::widgets::craft::text_edit(&mut self.hours_per_point).desired_width(60.0));
                                ui.end_row();
                                ui.label("Duration shown in:");
                                egui::ComboBox::from_id_salt("pi_unit").selected_text(format!("{:?}", self.unit)).show_ui(ui, |ui| {
                                    for u in [
                                        DurationUnit::Minutes,
                                        DurationUnit::Hours,
                                        DurationUnit::Days,
                                        DurationUnit::Weeks,
                                        DurationUnit::Months,
                                    ] {
                                        ui.selectable_value(&mut self.unit, u, format!("{u:?}"));
                                    }
                                });
                                ui.label("Currency symbol:");
                                ui.add(crate::widgets::craft::text_edit(&mut self.currency).desired_width(40.0));
                                ui.end_row();
                                ui.label("Critical if slack ≤");
                                ui.add(crate::widgets::craft::text_edit(&mut self.critical_slack).desired_width(60.0));
                                ui.label("Earned value uses:");
                                egui::ComboBox::from_id_salt("pi_ev_baseline")
                                    .selected_text(PlanSlot::Baseline(self.ev_baseline).name())
                                    .show_ui(ui, |ui| {
                                        for n in 0..=plancraft_core::baseline::MAX_BASELINE {
                                            ui.selectable_value(&mut self.ev_baseline, n, PlanSlot::Baseline(n).name());
                                        }
                                    });
                                ui.end_row();
                            });
                            ui.checkbox(&mut self.new_auto, "New tasks are auto scheduled");
                            ui.checkbox(
                                &mut self.use_res_cal,
                                "Schedule assigned tasks on their resources' calendars (time off delays work)",
                            );
                            ui.checkbox(&mut self.progress_from_time, "Derive % complete from tracked time");
                            ui.checkbox(&mut self.rules_on, "Stretch and percent-of-group (scheduling ruleset)");
                        },
                    );
                    if let Some(e) = &self.error {
                        ui.colored_label(theme::pal().ERROR, e);
                    }
                    ui.add_space(6.0);
                    let y = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), y, theme::hairline());
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = crate::widgets::craft::ITEM_GAP + 2.0;
                        if crate::widgets::craft::secondary_button(ui, crate::lang::text("common.cancel")).clicked() {
                            close = true;
                        }
                        if crate::widgets::craft::primary_button(ui, crate::lang::text("common.ok")).clicked() {
                            apply = true;
                        }
                    });
                });
        }
        if apply {
            match super::edit_bound(ws, handle, "Project Information", |p| self.apply(p)) {
                Ok(()) => close = true,
                Err(e) => self.error = Some(e),
            }
        }
        open && !close
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: OK wrote every field back as the dialog opened, undoing an agent's changes.
    #[test]
    fn ok_keeps_what_an_agent_changed_while_the_dialog_was_open() {
        let mut p = Project::new("Plan", plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        let mut info = ProjectInfo::new(&p);
        p.status_date = plancraft_core::dates::ymd(2026, 11, 2);
        p.manager = "Set by the agent".into();
        info.company = "Acme".into();
        info.apply(&mut p).unwrap();
        assert_eq!(p.company, "Acme");
        assert_eq!(p.manager, "Set by the agent");
        assert_eq!(p.status_date, plancraft_core::dates::ymd(2026, 11, 2));
    }
}
