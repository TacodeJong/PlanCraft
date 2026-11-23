//! Excel Gantt of the rows the current filter leaves visible, with the same
//! columns (and markings) the Gantt table shows — including group headers.

use plancraft_core::dates::Date;
use plancraft_core::prelude::*;
use plancraft_core::xlsx_gantt::{GanttBar, GanttIdentity, bar_color};
use plancraft_core::xlsx_out::XlsxCell;

use crate::columns::TaskCol;
use crate::fields::Field;
use crate::state::UiState;
use crate::views::{GroupRow, Row};
use crate::widgets::grid::Cell;

pub fn workbook(p: &Project, st: &UiState) -> Result<Vec<u8>, String> {
    // Same row set as the on-screen Gantt: filter, search, grouping, collapsed summaries.
    let view_rows = super::visible_rows(p, st);
    let fields = export_fields(p, st);
    let no_overallocation = |_: TaskId| false;
    let ctx = crate::columns::CellCtx { overallocated: &no_overallocation, others_on: None };
    let headers: Vec<String> = fields.iter().map(|f| f.title(p)).collect();

    let mut cells = Vec::new();
    let mut outlines = Vec::new();
    let mut bars = Vec::new();

    for row in &view_rows {
        match row {
            Row::Notes(_) => {}
            Row::Group(g) => {
                let (start, finish) = group_span(p, g);
                cells.push(group_identity(p, g, &fields));
                outlines.push(0);
                bars.push(match (start, finish) {
                    (Some(s), Some(f)) => Some(GanttBar {
                        start: s,
                        finish: f,
                        // Matches the on-screen group bracket (accent mid on a tinted row).
                        fill: [0x2D, 0x81, 0xFF],
                        milestone: false,
                        summary: true,
                    }),
                    _ => None,
                });
            }
            Row::Task(idx) => {
                let Some(task) = p.tasks.get(*idx) else { continue };
                let summary = p.has_children(*idx);
                let wash = task.row_mark.map(mark_wash_rgb);
                cells.push(
                    fields
                        .iter()
                        .enumerate()
                        .map(|(col, f)| {
                            let screen = f.cell(p, *idx, &ctx);
                            xlsx_from_cell(f, &screen, wash, summary, col == 0 || matches!(f, Field::Builtin(TaskCol::Name)))
                        })
                        .collect(),
                );
                outlines.push(u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(7).min(7));
                bars.push(task_bar(p, *idx, task, summary));
            }
        }
    }

    let identity = GanttIdentity { headers, cells, outlines };
    let scale = crate::state::Timescale::of_px(st.gantt.px_per_day).map(crate::state::Timescale::granularity);
    plancraft_core::xlsx_gantt::workbook(p, &identity, &bars, scale)
}

fn group_span(p: &Project, g: &GroupRow) -> (Option<Date>, Option<Date>) {
    let tasks = g.tasks.iter().filter_map(|&i| p.tasks.get(i));
    let start = tasks.clone().filter_map(|t| t.sched.start.map(|d| d.date())).min();
    let finish = tasks.filter_map(|t| t.sched.finish.map(|d| d.date())).max();
    (start, finish)
}

fn group_identity(p: &Project, g: &GroupRow, fields: &[Field]) -> Vec<XlsxCell> {
    // Soft header wash (paper), like the Swift report section shade.
    let header = [0xF2, 0xF2, 0xF2];
    fields
        .iter()
        .enumerate()
        .map(|(col, f)| {
            let screen = crate::views::gantt::group_cell(*f, p, g);
            let text = cell_text(f, &screen);
            let mut cell = XlsxCell::filled(text, true, header);
            if col == 0 || matches!(f, Field::Builtin(TaskCol::Name)) {
                cell.indent = 0;
            }
            cell
        })
        .collect()
}

fn task_bar(p: &Project, idx: usize, task: &Task, summary: bool) -> Option<GanttBar> {
    let start = task.sched.start?.date();
    let finish = task.sched.finish.map(|d| d.date()).unwrap_or(start);
    Some(GanttBar {
        start,
        finish,
        fill: bar_color(p, idx),
        milestone: task.is_milestone(),
        summary,
    })
}

fn xlsx_from_cell(field: &Field, screen: &Cell, wash: Option<[u8; 3]>, summary: bool, name_col: bool) -> XlsxCell {
    let text = cell_text(field, screen);
    let indent = if name_col {
        u8::try_from((screen.indent / 16.0).round() as i32).unwrap_or(0).min(8)
    } else {
        0
    };
    let bold = screen.bold || (summary && matches!(field, Field::Builtin(TaskCol::Name)));
    let fill = screen.bg.map(|c| [c.r(), c.g(), c.b()]).or(wash);
    match fill {
        Some(rgb) => {
            let mut cell = XlsxCell::filled(text, bold, rgb);
            cell.indent = indent;
            cell
        }
        None => XlsxCell::plain(text, bold, indent),
    }
}

/// Selected table columns, minus Indicators (icons only). Always keeps Name.
fn export_fields(p: &Project, st: &UiState) -> Vec<Field> {
    let mut fields: Vec<Field> = crate::fields::visible_fields(p, st.table, st.columns.get(st.table.label()))
        .into_iter()
        .filter(|f| *f != Field::Builtin(TaskCol::Indicators))
        .collect();
    if !fields.iter().any(|f| matches!(f, Field::Builtin(TaskCol::Name))) {
        fields.insert(0, Field::Builtin(TaskCol::Name));
    }
    if fields.is_empty() {
        fields.push(Field::Builtin(TaskCol::Name));
    }
    fields
}

fn cell_text(field: &Field, cell: &Cell) -> String {
    if !cell.text.is_empty() {
        return cell.text.clone();
    }
    if let Some(tip) = cell.tooltip.as_ref().filter(|s| !s.is_empty()) {
        return tip.clone();
    }
    if let Some(edit) = cell.edit_text.as_ref().filter(|s| !s.is_empty()) {
        return edit.clone();
    }
    let _ = field;
    String::new()
}

/// Soft mark wash on white paper (Excel is light regardless of the UI theme).
fn mark_wash_rgb(mark: plancraft_core::model::RowMark) -> [u8; 3] {
    let accent = match mark {
        plancraft_core::model::RowMark::Amber => [0xE0, 0x8A, 0x2E],
        plancraft_core::model::RowMark::Blue => [0x2D, 0x81, 0xFF],
        plancraft_core::model::RowMark::Pink => [0xE0, 0x4F, 0x5F],
        plancraft_core::model::RowMark::Green => [0x3D, 0xA3, 0x6F],
    };
    let t = 0.18f32;
    accent.map(|c| (255.0 * (1.0 - t) + (c as f32) * t).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::{self, TaskCol, TaskTable};
    use crate::state::{MarkFilter, UiState};
    use plancraft_core::model::RowMark;
    use plancraft_core::schedule::recalculate;

    #[test]
    fn export_keeps_selected_mark_columns_and_filtered_rows() {
        let mut p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        recalculate(&mut p);
        let pink = p.tasks.iter().find(|t| !t.sched.is_summary).map(|t| t.id).unwrap();
        columns::commit(TaskCol::RowMark, &mut p, pink, crate::lang::mark_name(RowMark::Pink)).unwrap();
        columns::commit(TaskCol::MarkNote, &mut p, pink, "watch").unwrap();
        recalculate(&mut p);

        let mut st = UiState::default();
        st.view = crate::state::ViewKind::Gantt;
        st.table = TaskTable::Entry;
        let choice = st.columns.entry(st.table.label().to_string()).or_default();
        choice.hidden = vec!["mode".into(), "leveling_delay".into(), "predecessors".into(), "resources".into()];
        choice.extra = vec!["row_mark".into(), "mark_note".into()];
        st.mark_filter = MarkFilter::Pink;

        let bytes = workbook(&p, &st).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(crate::lang::text("col.row_mark")), "selected mark column header");
        assert!(text.contains("watch"), "mark note exports");
        assert!(text.contains(crate::lang::mark_name(RowMark::Pink)));
        let pink_name = p.task(pink).unwrap().name.clone();
        assert!(text.contains(&pink_name));
        let other = p
            .tasks
            .iter()
            .find(|t| !t.sched.is_summary && t.id != pink && t.row_mark.is_none())
            .map(|t| t.name.clone())
            .unwrap();
        assert!(!text.contains(&other), "unmarked tasks stay out of a pink mark filter: {other}");
    }

    #[test]
    fn export_includes_group_headers_when_grouped() {
        let mut p = plancraft_core::sample::demo_project(plancraft_core::dates::ymd(2026, 10, 19).unwrap());
        recalculate(&mut p);
        let mut st = UiState::default();
        st.view = crate::state::ViewKind::Gantt;
        st.group = Some("status".into());

        let bytes = workbook(&p, &st).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        // Group headers look like "Not started  (N)" from group_cell.
        let has_group = crate::lang::status_name(TaskStatus::NotStarted);
        assert!(
            text.contains(has_group) && text.contains(" ("),
            "grouped export must show group header labels"
        );
    }
}
