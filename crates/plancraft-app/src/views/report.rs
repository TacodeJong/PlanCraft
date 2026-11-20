//! A filterable list of the plan: the rows the page filter and grouping leave visible.

use egui::{Rect, RichText, Sense, Ui, vec2};
use plancraft_core::prelude::*;
use plancraft_core::print::{Align, PrintCell, PrintColumn, PrintRow, RowKind, Sheet};
use plancraft_core::workspace::Document;

use crate::commands::Command;
use crate::state::UiState;
use crate::theme;
use crate::views::{self, Row};
use crate::widgets::date_picker;

fn titles(st: &UiState) -> [&'static str; 11] {
    [
        st.t("rpt.wbs"),
        st.t("rpt.name"),
        st.t("rpt.milestone"),
        st.t("rpt.start"),
        st.t("rpt.finish"),
        st.t("rpt.duration"),
        st.t("rpt.resources"),
        st.t("rpt.units"),
        st.t("rpt.predecessors"),
        st.t("rpt.percent"),
        st.t("rpt.status"),
    ]
}

struct Line {
    cells: [String; 11],
    milestone: bool,
    group: bool,
    mark: Option<RowMark>,
}

pub fn show(ui: &mut Ui, rect: Rect, doc: &mut Document, st: &mut UiState, _out: &mut Vec<Command>) {
    let lines = lines(&doc.project, st);
    let rect = super::filter_bar::task(ui, rect, &doc.project, st);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)));
    egui::ScrollArea::both().auto_shrink([false, false]).show(&mut child, |ui| {
        egui::Grid::new("report_grid").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
            for title in titles(st) {
                ui.label(RichText::new(title).strong().color(theme::pal().TEXT));
            }
            ui.end_row();
            for line in &lines {
                for (i, text) in line.cells.iter().enumerate() {
                    if i == 1 {
                        ui.horizontal(|ui| {
                            if let Some(mark) = line.mark {
                                let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                                ui.painter().circle_filled(rect.center(), 4.0, theme::row_mark_color(mark));
                            }
                            if line.group {
                                ui.label(RichText::new(text).strong().color(theme::pal().ACCENT));
                            } else {
                                ui.label(RichText::new(text).color(theme::pal().TEXT));
                            }
                        });
                    } else if i == 2 && line.milestone {
                        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                        let c = rect.center();
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![c + vec2(0.0, -5.0), c + vec2(5.0, 0.0), c + vec2(0.0, 5.0), c + vec2(-5.0, 0.0)],
                            theme::pal().MILESTONE,
                            egui::Stroke::NONE,
                        ));
                    } else {
                        ui.label(RichText::new(text).color(theme::pal().TEXT));
                    }
                }
                ui.end_row();
            }
        });
    });
}

/// The workbook of the filtered outline: a Report sheet with Excel row groups, a flat Data sheet,
/// and a Pivot sheet whose row fields are the Level columns and then `Taak (WBS)`.
pub fn workbook(p: &Project, st: &UiState) -> Result<Vec<u8>, String> {
    let idxs: Vec<usize> = views::outline_rows(p, st).into_iter().filter_map(|row| row.task()).collect();
    let report = report_sheet(p, &idxs, &titles(st).map(str::to_string));
    let data = data_sheet(p, &idxs);
    let range = plancraft_core::xlsx_out::used_range(&data);
    let fields = pivot_fields(&data);
    let has_leaves = data.rows.len() > 1;
    let pivot = plancraft_core::xlsx_out::XlsxSheet { name: "Pivot".into(), rows: Vec::new(), table: false, freeze: (0, 0) };
    let bytes = plancraft_core::xlsx_out::sheets(&[report, data, pivot])?;
    if !has_leaves {
        return Ok(bytes);
    }
    let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
    plancraft_core::pivot_xlsx::inject_pivot_table(bytes, "Data", &range, "Pivot", &refs, crate::lang::text("rpt.duration_days"))
        .map_err(|e| e.to_string())
}

/// The same rows, laid out for the print preview. Column widths sum to a landscape page.
pub fn sheet(p: &Project, st: &UiState) -> Sheet {
    let widths = [46.0, 144.0, 52.0, 72.0, 72.0, 58.0, 80.0, 46.0, 90.0, 58.0, 70.0];
    let columns =
        titles(st).iter().zip(widths).map(|(title, width)| PrintColumn { title: (*title).into(), width, align: Align::Left }).collect();
    let rows = lines(p, st)
        .into_iter()
        .map(|line| PrintRow {
            kind: if line.group { RowKind::Group { start: None, finish: None } } else { RowKind::Other },
            cells: line.cells.into_iter().map(PrintCell::text).collect(),
        })
        .collect();
    Sheet { title: p.name.clone(), columns, rows }
}

fn lines(p: &Project, st: &UiState) -> Vec<Line> {
    views::visible_rows(p, st)
        .into_iter()
        .filter_map(|row| match row {
            Row::Group(g) => Some(Line {
                cells: [
                    String::new(),
                    g.label,
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
                milestone: false,
                group: true,
                mark: None,
            }),
            Row::Notes(_) => None,
            Row::Task(idx) => {
                let t = p.tasks.get(idx)?;
                let wbs = p.wbs(idx);
                let wbs = if wbs.is_empty() { (idx.saturating_add(1)).to_string() } else { wbs };
                let day = |dt: Option<DateTime>| dt.map(date_picker::format_day).unwrap_or_default();
                Some(Line {
                    cells: [
                        wbs,
                        t.name.clone(),
                        if t.is_milestone() { crate::lang::text("common.yes").into() } else { String::new() },
                        day(t.sched.start.or(t.manual_start)),
                        day(t.sched.finish.or(t.manual_finish)),
                        p.format_task_duration(t),
                        p.format_resource_names_only(t.id),
                        p.format_resource_units(t.id),
                        p.format_predecessors(t.id),
                        t.percent_complete.to_string(),
                        crate::lang::status_name(p.displayed_status(idx)).into(),
                    ],
                    milestone: t.is_milestone(),
                    group: false,
                    mark: t.row_mark,
                })
            }
        })
        .collect()
}

fn report_sheet(p: &Project, idxs: &[usize], headers: &[String; 11]) -> plancraft_core::xlsx_out::XlsxSheet {
    let mut rows = vec![header_row(headers)];
    for &idx in idxs {
        let Some(t) = p.tasks.get(idx) else { continue };
        let depth = t.outline_level.saturating_sub(1).min(7);
        let summary = p.has_children(idx);
        let wbs = p.wbs(idx);
        let wbs = if wbs.is_empty() { idx.saturating_add(1).to_string() } else { wbs };
        let day = |dt: Option<DateTime>| dt.map(date_picker::format_day).unwrap_or_default();
        let values = [
            wbs,
            t.name.clone(),
            if t.is_milestone() { crate::lang::text("common.yes").into() } else { String::new() },
            day(t.sched.start.or(t.manual_start)),
            day(t.sched.finish.or(t.manual_finish)),
            p.format_task_duration(t),
            p.format_resource_names_only(t.id),
            p.format_resource_units(t.id),
            p.format_predecessors(t.id),
            t.percent_complete.to_string(),
            crate::lang::status_name(p.displayed_status(idx)).into(),
        ];
        let cells = values
            .into_iter()
            .enumerate()
            .map(|(col, text)| {
                let indent = if col == 1 { u8::try_from(depth).unwrap_or(7) } else { 0 };
                plancraft_core::xlsx_out::XlsxCell::plain(text, summary, indent)
            })
            .collect();
        rows.push(plancraft_core::xlsx_out::XlsxRow { cells, outline: u8::try_from(depth).unwrap_or(7) });
    }
    plancraft_core::xlsx_out::XlsxSheet { name: "Report".into(), rows, table: false, freeze: (0, 0) }
}

fn data_sheet(p: &Project, idxs: &[usize]) -> plancraft_core::xlsx_out::XlsxSheet {
    let leaves: Vec<usize> = idxs.iter().copied().filter(|&i| !p.has_children(i)).collect();
    let chains: Vec<Vec<String>> = leaves.iter().copied().map(|i| ancestors(p, i)).collect();
    let levels = chains.iter().map(Vec::len).max().unwrap_or(0);
    let mut titles = Vec::new();
    let level = crate::lang::text("rpt.level");
    for n in 1..=levels {
        titles.push(format!("{level} {n}"));
    }
    titles.extend(
        [
            "WBS",
            "Name",
            crate::lang::text("rpt.task_wbs"),
            "Milestone",
            "Start",
            "Finish",
            crate::lang::text("rpt.duration_days"),
            "Resources",
            "Units",
            "% complete",
            "Status",
        ]
        .map(str::to_string),
    );
    let mut rows = vec![header_row(&titles)];
    for (idx, chain) in leaves.iter().copied().zip(chains) {
        let Some(t) = p.tasks.get(idx) else { continue };
        let last = chain.last().cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| t.name.clone());
        let mut cells = Vec::new();
        for n in 0..levels {
            let value = chain.get(n).cloned().filter(|s| !s.is_empty()).unwrap_or_else(|| last.clone());
            cells.push(plancraft_core::xlsx_out::XlsxCell::text(value));
        }
        let wbs = p.wbs(idx);
        let wbs = if wbs.is_empty() { idx.saturating_add(1).to_string() } else { wbs };
        let day = |dt: Option<DateTime>| dt.map(date_picker::format_day).unwrap_or_default();
        let taak = format!("{wbs} {}", t.name);
        for text in [
            wbs,
            t.name.clone(),
            taak,
            if t.is_milestone() { crate::lang::text("common.yes").into() } else { String::new() },
            day(t.sched.start.or(t.manual_start)),
            day(t.sched.finish.or(t.manual_finish)),
        ] {
            cells.push(plancraft_core::xlsx_out::XlsxCell::text(text));
        }
        cells.push(plancraft_core::xlsx_out::XlsxCell::number(duration_days(p, t)));
        for text in [
            p.format_resource_names_only(t.id),
            p.format_resource_units(t.id),
            t.percent_complete.to_string(),
            p.displayed_status(idx).label().into(),
        ] {
            cells.push(plancraft_core::xlsx_out::XlsxCell::text(text));
        }
        rows.push(plancraft_core::xlsx_out::XlsxRow { cells, outline: 0 });
    }
    plancraft_core::xlsx_out::XlsxSheet { name: "Data".into(), rows, table: true, freeze: (0, 0) }
}

/// Level columns, outermost first, then `Taak (WBS)` as the leaf row field.
fn pivot_fields(sheet: &plancraft_core::xlsx_out::XlsxSheet) -> Vec<String> {
    let mut names = Vec::new();
    let Some(header) = sheet.rows.first() else { return names };
    for cell in &header.cells {
        let prefix = format!("{} ", crate::lang::text("rpt.level"));
        if cell.text.starts_with(&prefix) {
            names.push(cell.text.clone());
        }
    }
    let task = crate::lang::text("rpt.task_wbs");
    if header.cells.iter().any(|c| c.text == task) {
        names.push(task.into());
    }
    names
}

fn header_row(titles: &[String]) -> plancraft_core::xlsx_out::XlsxRow {
    plancraft_core::xlsx_out::XlsxRow {
        cells: titles.iter().cloned().map(|t| plancraft_core::xlsx_out::XlsxCell::plain(t, true, 0)).collect(),
        outline: 0,
    }
}

fn ancestors(p: &Project, idx: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = p.parent_index(idx);
    while let Some(i) = cursor {
        if let Some(t) = p.tasks.get(i) {
            names.push(t.name.clone());
        }
        cursor = p.parent_index(i);
    }
    names.reverse();
    names
}

fn duration_days(p: &Project, t: &Task) -> String {
    let per = p.settings.time.minutes_per_day.max(1);
    let mins = t.sched.duration.max(0);
    let whole = mins / per;
    let rem = mins.rem_euclid(per);
    if rem == 0 {
        whole.to_string()
    } else {
        let tenths = rem.saturating_mul(10) / per;
        format!("{whole}.{tenths}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tasks_export_contains_header_and_names() {
        let mut p = Project::new("Report", plancraft_core::dates::ymd(2026, 10, 15).unwrap());
        p.add_task("Design", None);
        p.add_task("Build", None);
        plancraft_core::schedule::recalculate(&mut p);
        let bytes = workbook(&p, &UiState::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Name"), "{text}");
        assert!(text.contains("Design"), "{text}");
        assert!(text.contains("Build"), "{text}");
        assert!(text.contains("sheet1.xml") || text.contains("<sheetData"), "{text}");
    }

    /// The workbook carries the names and the units as their own columns, no `[50%]` suffixes.
    #[test]
    fn the_workbook_splits_resource_names_and_units() {
        let mut p = Project::new("Units", plancraft_core::dates::ymd(2026, 11, 16).unwrap());
        let id = p.add_task("Design", None);
        p.apply_resource_names(id, "Ana[50%], Ben").unwrap();
        plancraft_core::schedule::recalculate(&mut p);
        let bytes = workbook(&p, &UiState::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Ana, Ben"), "{text}");
        assert!(text.contains("50%, 100%"), "{text}");
        assert!(!text.contains("Ana[50%]"), "{text}");
        assert!(text.contains(crate::lang::text("rpt.units")), "{text}");
    }

    #[test]
    fn summary_children_are_grouped_and_listed_under_level_1() {
        let mut p = Project::new("Hierarchy", plancraft_core::dates::ymd(2026, 10, 15).unwrap());
        let _phase = p.add_task("Phase", None);
        let design = p.add_task("Design", None);
        let _other = p.add_task("Other", None);
        let mid = p.add_task("Mid", None);
        let build = p.add_task("Build", None);
        p.task_mut(design).unwrap().outline_level = 2;
        p.task_mut(mid).unwrap().outline_level = 2;
        p.task_mut(build).unwrap().outline_level = 3;
        plancraft_core::schedule::recalculate(&mut p);
        let bytes = workbook(&p, &UiState::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.matches("outlineLevel=\"1\"").count() >= 2, "{text}");
        assert!(text.contains("summaryBelow=\"0\""), "{text}");
        let level = crate::lang::text("rpt.level");
        assert!(text.contains(&format!("{level} 1")), "{text}");
        assert!(text.contains(&format!("{level} 2")), "{text}");
        assert!(text.contains(crate::lang::text("rpt.task_wbs")), "{text}");
        assert!(text.contains(crate::lang::text("rpt.duration_days")), "{text}");
        assert!(text.matches("Phase").count() >= 3, "report row plus both level cells of the shallow leaf: {text}");
        assert!(text.contains("displayName=\"Data\""), "{text}");
        assert!(text.contains("xl/pivotTables/pivotTable1.xml"), "{text}");
        // The depth-2 leaf repeats its group in Level 2, so neither level cell is empty.
        assert!(text.contains(r#"<c r="A2" s="0" t="inlineStr"><is><t>Phase</t></is></c>"#), "{text}");
        assert!(text.contains(r#"<c r="B2" s="0" t="inlineStr"><is><t>Phase</t></is></c>"#), "{text}");
        assert!(text.contains(r#"<c r="A3" s="0" t="inlineStr"><is><t>Other</t></is></c>"#), "{text}");
        assert!(text.contains(r#"<c r="B3" s="0" t="inlineStr"><is><t>Mid</t></is></c>"#), "{text}");
        assert!(text.contains("1.1 Design"), "{text}");
        assert!(text.contains("2.1.1 Build"), "{text}");
        assert!(!text.contains(r#"<c r="A2" s="0" t="inlineStr"><is><t></t></is></c>"#));
        assert!(!text.contains(r#"<c r="B2" s="0" t="inlineStr"><is><t></t></is></c>"#));
        assert!(!text.contains(r#"<c r="A3" s="0" t="inlineStr"><is><t></t></is></c>"#));
        assert!(!text.contains(r#"<c r="B3" s="0" t="inlineStr"><is><t></t></is></c>"#));
    }
}
