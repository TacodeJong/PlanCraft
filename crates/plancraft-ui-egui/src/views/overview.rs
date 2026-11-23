//! Agenda dashboard around the status moment: KPIs, day strip, and bucket lists (ArtCraft).

use egui::{Color32, FontId, Frame, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use plancraft_core::agenda::{self, AgendaBucket};
use plancraft_core::prelude::*;
use plancraft_core::report;
use plancraft_core::workspace::Document;

use crate::commands::Command;
use crate::edit::EditExt;
use crate::format;
use crate::state::{ToastKind, UiState, ViewKind};
use crate::theme;
use crate::widgets::craft::{self, CORNER};

const KPI_GAP: f32 = 8.0;
const KPI_MARGIN_X: i8 = 12;
const KPI_MARGIN_Y: i8 = 10;
const KPI_STROKE: f32 = 1.0;
const DAY_CELL_H: f32 = 64.0;
const DAY_CELL_W: f32 = 56.0;

/// KPI grid columns from the width available to the metric row.
pub(crate) fn metric_columns(available_width: f32) -> usize {
    if available_width >= 880.0 {
        4
    } else if available_width >= 460.0 {
        2
    } else {
        1
    }
}

/// Outer width for each KPI frame so margins, stroke, and gaps fit in `available_width`.
pub(crate) fn metric_outer_width(available_width: f32, cols: usize) -> f32 {
    let gaps = KPI_GAP * cols.saturating_sub(1) as f32;
    ((available_width - gaps) / cols.max(1) as f32).max(0.0)
}

/// Last painted Overview geometry (test harness reads this; not a product API).
#[derive(Clone, Debug)]
#[allow(dead_code)] // fields are asserted from `ui_tests` via `last_geom`
pub(crate) struct OverviewGeom {
    pub viewport: Rect,
    pub kpi_frames: Vec<Rect>,
    pub day_strip: Rect,
    pub agenda_heading: Option<Rect>,
    pub open_gantt: Option<Rect>,
    pub heading: Option<Rect>,
    pub filter_shown: usize,
    pub filter_total: usize,
}

impl Default for OverviewGeom {
    fn default() -> Self {
        Self {
            viewport: Rect::NOTHING,
            kpi_frames: Vec::new(),
            day_strip: Rect::NOTHING,
            agenda_heading: None,
            open_gantt: None,
            heading: None,
            filter_shown: 0,
            filter_total: 0,
        }
    }
}

#[cfg(test)]
pub(crate) fn last_geom(ctx: &egui::Context) -> Option<OverviewGeom> {
    ctx.data(|d| d.get_temp::<OverviewGeom>(egui::Id::unique("overview_geom")))
}

fn store_geom(ctx: &egui::Context, geom: OverviewGeom) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("overview_geom"), geom));
}

pub fn show(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState, out: &mut Vec<Command>) {
    let items = agenda::items(&doc.project);
    let resource = st.agenda_resource.clone();
    let tiles = agenda::tiles(&doc.project, resource.as_deref());
    let today = plancraft_core::dates::today();
    let status_at = report::status_moment(&doc.project);
    let strip = agenda::strip(&doc.project, resource.as_deref(), today);
    let day = st.agenda_day;

    // Filter-bar counts must match the rows actually listed (day strip = starts/dues/milestones).
    let (filter_shown, filter_total, day_rows, shown) = if let Some(d) = day {
        let all = agenda::on_day(&doc.project, None, d);
        let filtered: Vec<_> = all.iter().filter(|item| super::filter_bar::agenda_matches(&doc.project, st, item.id)).cloned().collect();
        (filtered.len(), all.len(), Some(filtered), Vec::new())
    } else {
        let shown: Vec<_> = items.iter().filter(|item| super::filter_bar::agenda_matches(&doc.project, st, item.id)).cloned().collect();
        (shown.len(), items.len(), None, shown)
    };
    let rect = super::filter_bar::overview(ui, rect, &doc.project, st, filter_shown, filter_total);

    let content = rect.shrink(12.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
    // Clip painting/hits to the Overview rect (intersect inherited clip), not Properties.
    child.shrink_clip_rect(content);
    let ui = &mut child;

    let mut geom = OverviewGeom { viewport: content, filter_shown, filter_total, ..OverviewGeom::default() };

    egui::ScrollArea::vertical().id_salt("overview_page").auto_shrink([false, false]).show(ui, |ui| {
        header(ui, doc, st, status_at, today, out, &mut geom);
        ui.add_space(12.0);
        metrics(ui, st, tiles, &mut geom);
        ui.add_space(14.0);
        craft::section_label(ui, st.t("ov.days"));
        ui.add_space(4.0);
        day_strip(ui, st, &strip, today, &mut geom);
        ui.add_space(12.0);

        if let Some(rows) = day_rows {
            if let Some(d) = day {
                craft::section_label(ui, &format::day(d));
            }
            ui.add_space(4.0);
            if rows.is_empty() {
                empty_line(ui, st.t("ov.day_empty"));
            } else {
                for item in rows {
                    day_row(ui, doc, st, &item);
                    ui.add_space(4.0);
                }
            }
            store_geom(ui.ctx(), geom);
            return;
        }

        let agenda_resp = craft::section_label(ui, st.t("ov.agenda"));
        geom.agenda_heading = Some(agenda_resp.rect);
        ui.add_space(4.0);
        if items.is_empty() {
            empty_line(ui, st.t("ov.empty"));
            store_geom(ui.ctx(), geom);
            return;
        }
        if shown.is_empty() {
            empty_line(ui, st.t("ov.filtered_empty"));
            store_geom(ui.ctx(), geom);
            return;
        }
        for bucket in [AgendaBucket::Overdue, AgendaBucket::Today, AgendaBucket::ThisWeek, AgendaBucket::Upcoming] {
            ui.add_space(8.0);
            craft::section_label(ui, bucket_name(st, bucket));
            ui.add_space(2.0);
            let rows: Vec<_> = shown.iter().filter(|item| item.bucket == bucket).cloned().collect();
            if rows.is_empty() {
                empty_line(ui, st.t("ov.none"));
                continue;
            }
            for item in rows {
                agenda_row(ui, doc, st, &item);
                ui.add_space(4.0);
            }
        }
        store_geom(ui.ctx(), geom);
    });
}

fn header(
    ui: &mut Ui,
    doc: &Document,
    st: &UiState,
    status_at: DateTime,
    today: plancraft_core::dates::Date,
    out: &mut Vec<Command>,
    geom: &mut OverviewGeom,
) {
    ui.horizontal_wrapped(|ui| {
        let title = RichText::new(st.t("view.overview")).font(FontId::new(22.0, theme::display())).color(theme::pal().TEXT);
        let heading = ui.label(title);
        geom.heading = Some(heading.rect);
        ui.add_space(8.0);
        // Keep Open Gantt reachable: truncate a long project name within the remaining row budget.
        let gantt_reserve = 120.0;
        let name_budget = (ui.available_width() - gantt_reserve).max(48.0);
        ui.allocate_ui_with_layout(
            vec2(name_budget.min(ui.available_width()), 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add(egui::Label::new(RichText::new(&doc.project.name).color(theme::pal().TEXT_WEAK)).truncate());
            },
        );
        let label = RichText::new(st.t("ov.open_gantt")).color(theme::pal().INVERT_FG);
        let btn = egui::Button::new(label).fill(theme::pal().INVERT_BG).corner_radius(CORNER).min_size(vec2(0.0, craft::CONTROL_H - 4.0));
        let resp = ui.add(btn).on_hover_cursor(egui::CursorIcon::PointingHand);
        geom.open_gantt = Some(resp.rect);
        if resp.clicked() {
            out.push(Command::SetView(ViewKind::Gantt));
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        craft::hud(ui, st.t("ov.status_as_of"), theme::pal().TEXT_WEAK);
        ui.label(RichText::new(format::date_long(Some(status_at))).color(theme::pal().TEXT));
        ui.label(RichText::new("·").color(theme::pal().LINE_STRONG));
        craft::hud(ui, st.t("ov.calendar_today"), theme::pal().TEXT_WEAK);
        ui.label(RichText::new(format::weekday_date(today)).color(theme::pal().TEXT));
    });
    ui.add_space(6.0);
    ui.label(RichText::new(st.t("ov.around")).color(theme::pal().TEXT_WEAK));
    let y = ui.cursor().top() + 4.0;
    ui.painter().hline(ui.max_rect().x_range(), y, theme::hairline());
    ui.add_space(10.0);
}

fn metrics(ui: &mut Ui, st: &UiState, tiles: agenda::Tiles, geom: &mut OverviewGeom) {
    let width = ui.available_width();
    let cols = metric_columns(width);
    let outer_w = metric_outer_width(width, cols);
    let entries = [
        (tiles.overdue, st.t("ov.overdue"), MetricTone::Overdue),
        (tiles.this_week, st.t("ov.this_week"), MetricTone::Neutral),
        (tiles.milestones, st.t("ov.milestones"), MetricTone::Neutral),
        (tiles.waiting, st.t("ov.waiting"), MetricTone::Waiting),
    ];
    for row in entries.chunks(cols) {
        ui.horizontal_top(|ui| {
            // Own the gap budget; do not also pay egui's default item_spacing.
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, &(n, label, tone)) in row.iter().enumerate() {
                if i > 0 {
                    ui.add_space(KPI_GAP);
                }
                let frame = metric_tile(ui, outer_w, n, label, tone);
                geom.kpi_frames.push(frame);
            }
        });
        ui.add_space(KPI_GAP);
    }
}

#[derive(Clone, Copy)]
enum MetricTone {
    Neutral,
    Overdue,
    Waiting,
}

fn metric_tile(ui: &mut Ui, outer_w: f32, n: usize, label: &str, tone: MetricTone) -> Rect {
    let color = match (tone, n > 0) {
        (MetricTone::Overdue, true) => theme::pal().ERROR,
        (MetricTone::Waiting, true) => theme::pal().WARN,
        _ => theme::pal().TEXT,
    };
    let outer_h = 64.0 + 2.0 * (f32::from(KPI_MARGIN_Y) + KPI_STROKE);
    let inner_w = (outer_w - 2.0 * (f32::from(KPI_MARGIN_X) + KPI_STROKE)).max(0.0);
    let inner = ui.allocate_ui_with_layout(vec2(outer_w, outer_h), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_width(outer_w);
        ui.set_max_width(outer_w);
        Frame::new()
            .fill(theme::pal().WASH)
            .stroke(Stroke::new(KPI_STROKE, theme::pal().LINE))
            .corner_radius(0.0)
            .inner_margin(Margin::symmetric(KPI_MARGIN_X, KPI_MARGIN_Y))
            .show(ui, |ui| {
                ui.set_min_height(64.0);
                ui.set_min_width(inner_w);
                ui.set_max_width(inner_w);
                kpi_caption(ui, label);
                ui.add_space(4.0);
                ui.label(RichText::new(n.to_string()).font(FontId::new(22.0, theme::display())).color(color));
            })
            .response
            .rect
    });
    inner.inner
}

/// HUD caption clipped to the tile so long labels cannot expand the Frame past the outer budget.
fn kpi_caption(ui: &mut Ui, label: &str) {
    let color = theme::pal().TEXT_WEAK;
    let galley = craft::caps(ui.painter(), label, theme::HUD_SIZE, theme::mono(), theme::HUD_SPACING, color);
    let max_w = ui.available_width().max(1.0);
    let size = vec2(galley.size().x.min(max_w), galley.size().y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().with_clip_rect(rect).galley(rect.min, galley, color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
}

fn day_strip(ui: &mut Ui, st: &mut UiState, strip: &[agenda::DayCount], today: plancraft_core::dates::Date, geom: &mut OverviewGeom) {
    // Bound height to the day cells (+ scrollbar); do not eat the remaining vertical page.
    let bar = ui.spacing().scroll.bar_width.max(8.0);
    let out = egui::ScrollArea::horizontal().id_salt("overview_days").max_height(DAY_CELL_H + bar + 4.0).auto_shrink([false, true]).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                for cell in strip {
                    let on = st.agenda_day == Some(cell.date);
                    let weekend = plancraft_core::dates::weekday_index(cell.date.weekday()) >= 5;
                    day_cell(ui, st, cell, on, weekend, cell.date == today);
                }
            });
        },
    );
    geom.day_strip = out.inner_rect;
}

fn day_cell(ui: &mut Ui, st: &mut UiState, cell: &agenda::DayCount, on: bool, weekend: bool, is_today: bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(DAY_CELL_W, DAY_CELL_H), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CORNER, theme::pal().INVERT_BG);
    } else if resp.hovered() {
        p.rect_filled(rect, CORNER, theme::pal().WASH_HOVER);
        p.rect_stroke(rect, CORNER, Stroke::new(1.0, theme::pal().LINE), StrokeKind::Inside);
    } else if is_today {
        p.rect_filled(rect, CORNER, theme::pal().WASH_SELECTED);
        p.rect_stroke(rect, CORNER, Stroke::new(1.0, theme::pal().LINE_STRONG), StrokeKind::Inside);
    } else {
        p.rect_stroke(rect, CORNER, Stroke::new(1.0, theme::pal().LINE), StrokeKind::Inside);
    }
    let ink = if on {
        theme::pal().INVERT_FG
    } else if weekend {
        theme::pal().TEXT_WEAK
    } else {
        theme::pal().TEXT
    };
    let muted = if on { theme::pal().INVERT_FG.gamma_multiply(0.7) } else { theme::pal().TEXT_WEAK };
    let wd = weekday_short(cell.date).to_uppercase();
    let day = cell.date.day().to_string();
    let counts = format!("{} · {}", cell.starting, cell.due.saturating_add(cell.milestones));
    let galley_wd = p.layout_no_wrap(wd, FontId::new(theme::HUD_SIZE, theme::mono()), muted);
    let galley_day = p.layout_no_wrap(day, FontId::new(16.0, theme::semibold()), ink);
    let galley_c = p.layout_no_wrap(counts, FontId::proportional(10.0), muted);
    let top = egui::pos2(rect.center().x - galley_wd.size().x / 2.0, rect.top() + 6.0);
    let mid = egui::pos2(rect.center().x - galley_day.size().x / 2.0, rect.center().y - galley_day.size().y / 2.0);
    let bot = egui::pos2(rect.center().x - galley_c.size().x / 2.0, rect.bottom() - galley_c.size().y - 6.0);
    p.galley(top, galley_wd, muted);
    p.galley(mid, galley_day, ink);
    p.galley(bot, galley_c, muted);
    let tip = format!(
        "{} {}  ·  {} {}  ·  {} {}",
        cell.starting,
        st.t("ov.starting"),
        cell.due,
        st.t("ov.due"),
        cell.milestones,
        st.t("ov.milestones")
    );
    let resp = resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        st.agenda_day = if on { None } else { Some(cell.date) };
    }
}

fn agenda_row(ui: &mut Ui, doc: &mut Document, st: &mut UiState, item: &agenda::AgendaItem) {
    let selected = st.sel.tasks.contains(&item.id);
    let mark = item.row_mark;
    let fill = if selected { theme::pal().WASH_SELECTED } else { theme::pal().WASH };
    let stroke = if selected { theme::pal().LINE_STRONG } else { theme::pal().LINE };
    let mut clicked = false;
    let resp = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke))
        .corner_radius(0.0)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                mark_dot(ui, mark);
                let name = RichText::new(&item.name).strong().color(theme::pal().TEXT);
                if ui.add(egui::Label::new(name).sense(Sense::click())).clicked() {
                    clicked = true;
                }
                ui.label(RichText::new(format::date(Some(item.finish))).color(theme::pal().TEXT_WEAK));
                if item.late {
                    ui.label(RichText::new(st.t("ov.late")).color(theme::pal().ERROR));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    row_mark_combo(ui, doc, st, item.id, mark);
                });
            });
        })
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if clicked || resp.interact(Sense::click()).clicked() {
        st.sel.select_task(item.id);
    }
}

fn day_row(ui: &mut Ui, doc: &Document, st: &mut UiState, item: &agenda::DayItem) {
    let mark = doc.project.task(item.id).and_then(|t| t.row_mark);
    let selected = st.sel.tasks.contains(&item.id);
    let fill = if selected { theme::pal().WASH_SELECTED } else { theme::pal().WASH };
    let mut clicked = false;
    let resp = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, theme::pal().LINE))
        .corner_radius(0.0)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                mark_dot(ui, mark);
                if item.milestone {
                    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    let c = rect.center();
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![c + vec2(0.0, -5.0), c + vec2(5.0, 0.0), c + vec2(0.0, 5.0), c + vec2(-5.0, 0.0)],
                        theme::pal().TEXT,
                        Stroke::NONE,
                    ));
                }
                let name = RichText::new(&item.name).strong().color(theme::pal().TEXT);
                if ui.add(egui::Label::new(name).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    clicked = true;
                }
                let kind = if item.milestone {
                    st.t("ov.milestones")
                } else if item.due {
                    st.t("ov.due")
                } else {
                    st.t("ov.starting")
                };
                ui.label(RichText::new(kind).color(theme::pal().TEXT_WEAK));
            });
        })
        .response
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // Body selectable like agenda rows; mark/kind/milestone remain ordinary content hits.
    if clicked || resp.interact(Sense::click()).clicked() {
        st.sel.select_task(item.id);
    }
}

fn row_mark_combo(ui: &mut Ui, doc: &mut Document, st: &mut UiState, id: TaskId, mark: Option<RowMark>) {
    let none = st.t("ov.none");
    let current = mark.map(RowMark::label).unwrap_or(none);
    egui::ComboBox::from_id_salt(("overview_mark", id)).width(96.0).selected_text(current).show_ui(ui, |ui| {
        let choices = [None].into_iter().chain(RowMark::ALL.into_iter().map(Some));
        for choice in choices {
            let label = choice.map(RowMark::label).unwrap_or(none);
            if !ui.selectable_label(mark == choice, label).clicked() {
                continue;
            }
            if mark == choice {
                continue;
            }
            if let Err(e) = doc.edit_msg("Row mark", move |p| {
                if let Some(t) = p.task_mut(id) {
                    t.row_mark = choice;
                    if choice.is_none() {
                        t.mark_note.clear();
                    }
                }
                Ok(())
            }) {
                st.toast(ToastKind::Error, e);
            }
        }
    });
}

fn mark_dot(ui: &mut Ui, mark: Option<RowMark>) {
    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(dot.center(), 4.0, mark.map(mark_color).unwrap_or(theme::pal().LINE_STRONG));
}

fn empty_line(ui: &mut Ui, text: &str) {
    Frame::new().fill(theme::pal().WASH).stroke(Stroke::new(1.0, theme::pal().LINE)).inner_margin(Margin::symmetric(10, 8)).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(theme::pal().TEXT_WEAK));
        },
    );
}

fn weekday_short(date: plancraft_core::dates::Date) -> &'static str {
    plancraft_core::dates::weekday_short(date.weekday())
}

fn bucket_name(st: &UiState, bucket: AgendaBucket) -> &'static str {
    match bucket {
        AgendaBucket::Overdue => st.t("ov.overdue"),
        AgendaBucket::Today => st.t("ov.today"),
        AgendaBucket::ThisWeek => st.t("ov.this_week"),
        AgendaBucket::Upcoming => st.t("ov.upcoming"),
    }
}

fn mark_color(mark: RowMark) -> Color32 {
    match mark {
        RowMark::Amber => theme::pal().WARN,
        RowMark::Blue => theme::pal().ACCENT,
        RowMark::Pink => theme::pal().ERROR,
        RowMark::Green => theme::pal().CHIP_OK,
    }
}

#[cfg(test)]
mod tests {
    use super::{metric_columns, metric_outer_width};

    #[test]
    fn metric_columns_follow_available_width() {
        assert_eq!(metric_columns(1200.0), 4);
        assert_eq!(metric_columns(880.0), 4);
        assert_eq!(metric_columns(700.0), 2);
        assert_eq!(metric_columns(460.0), 2);
        assert_eq!(metric_columns(320.0), 1);
    }

    #[test]
    fn metric_outer_width_fits_gaps_inside_budget() {
        let width = 400.0;
        let cols = 2;
        let outer = metric_outer_width(width, cols);
        assert!((outer * 2.0 + 8.0 - width).abs() < 0.01, "two tiles + gap fill the row: {outer}");
        let four = metric_outer_width(900.0, 4);
        assert!((four * 4.0 + 8.0 * 3.0 - 900.0).abs() < 0.01);
    }

    #[test]
    fn metric_outer_width_keeps_small_available_widths() {
        assert!((metric_outer_width(80.0, 1) - 80.0).abs() < 0.01);
    }
}
