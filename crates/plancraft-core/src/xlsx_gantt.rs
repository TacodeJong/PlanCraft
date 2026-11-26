//! A Gantt chart as an Excel sheet: one row per exported line and one filled cell
//! per period the bar covers. Identity columns and bars come from the caller (the
//! view's selected columns, filter and group headers). Without a timescale, days
//! when the span fits in about three months, weeks when it does not.

use crate::dates::prelude::*;
use crate::dates::{self, Date};
use crate::model::Project;
use crate::schedule;
use crate::workload::Granularity;
use crate::xlsx_out::{self, XlsxCell, XlsxRow, XlsxSheet};

struct Period {
    start: Date,
    end: Date,
    label: String,
}

/// Left-hand columns before the time-phased fills.
pub struct GanttIdentity {
    pub headers: Vec<String>,
    /// One cell list per body row (same order as [`GanttBar`] options).
    pub cells: Vec<Vec<XlsxCell>>,
    /// Excel outline level per body row (0–7).
    pub outlines: Vec<u8>,
}

/// Chart bar for one body row. `None` in the parallel slice means no fill.
pub struct GanttBar {
    pub start: Date,
    pub finish: Date,
    pub fill: [u8; 3],
    pub milestone: bool,
    /// Summary / group header: lighter fill, like the on-screen bracket.
    pub summary: bool,
}

/// `identity` and `bars` are parallel. `scale` is the Q/M/D timescale the chart
/// showed (`None` keeps the automatic day/week choice).
pub fn workbook(p: &Project, identity: &GanttIdentity, bars: &[Option<GanttBar>], scale: Option<Granularity>) -> Result<Vec<u8>, String> {
    if identity.cells.len() != bars.len() || identity.outlines.len() != bars.len() {
        return Err("gantt identity rows and bars must match".into());
    }
    let freeze_cols = u32::try_from(identity.headers.len()).unwrap_or(0);
    let periods = periods(p, bars, scale);
    let mut sheet_rows = Vec::with_capacity(bars.len().saturating_add(1));
    let mut header: Vec<XlsxCell> = identity.headers.iter().map(|w| XlsxCell::plain(w.clone(), true, 0)).collect();
    for period in &periods {
        header.push(XlsxCell::plain(&period.label, true, 0));
    }
    sheet_rows.push(XlsxRow { cells: header, outline: 0 });
    for (i, bar) in bars.iter().enumerate() {
        let mut cells = identity.cells.get(i).cloned().unwrap_or_default();
        cells.resize_with(identity.headers.len(), || XlsxCell::text(""));
        let fill_row = |task_fill: [u8; 3], summary: bool, milestone: bool, span: (Date, Date)| {
            let fill = if summary { lighter(task_fill) } else { task_fill };
            periods.iter().map(|period| period_cell(span, period, fill, summary, milestone)).collect::<Vec<_>>()
        };
        match bar {
            Some(b) => cells.extend(fill_row(b.fill, b.summary, b.milestone, (b.start, b.finish))),
            None => cells.extend(periods.iter().map(|_| XlsxCell::text(""))),
        }
        let outline = identity.outlines.get(i).copied().unwrap_or(0).min(7);
        sheet_rows.push(XlsxRow { cells, outline });
    }
    xlsx_out::sheets(&[XlsxSheet { name: "Gantt".into(), rows: sheet_rows, table: false, freeze: (freeze_cols, 1) }])
}

fn period_cell(span: (Date, Date), period: &Period, fill: [u8; 3], summary: bool, milestone: bool) -> XlsxCell {
    let (start, finish) = span;
    if start > period.end || finish < period.start {
        return XlsxCell::text("");
    }
    if milestone {
        let mark = if start >= period.start && start <= period.end { "◆" } else { "" };
        return XlsxCell::filled(mark, false, fill);
    }
    XlsxCell::filled("", summary, fill)
}

fn periods(p: &Project, bars: &[Option<GanttBar>], scale: Option<Granularity>) -> Vec<Period> {
    let mut first = None;
    let mut last = None;
    for bar in bars.iter().flatten() {
        first = Some(first.map_or(bar.start, |d: Date| d.min(bar.start)));
        last = Some(last.map_or(bar.finish, |d: Date| d.max(bar.finish)));
    }
    let (Some(first), Some(last)) = (first, last) else { return Vec::new() };
    match scale {
        Some(Granularity::Day) => working_days(p, first, last),
        Some(Granularity::Week) => weeks(first, last),
        Some(g @ (Granularity::Month | Granularity::Quarter)) => buckets(g, first, last),
        None if dates::days_between(first, last) > 90 => weeks(first, last),
        None => working_days(p, first, last),
    }
}

fn buckets(g: Granularity, first: Date, last: Date) -> Vec<Period> {
    g.buckets(first, last)
        .into_iter()
        .map(|start| {
            let end = dates::shift_days(g.next(start), -1);
            let label = match g {
                Granularity::Quarter => format!("Q{} {}", start.month0() / 3 + 1, start.year32()),
                _ => format!("{:02}-{}", start.month32(), start.year32()),
            };
            Period { start, end, label }
        })
        .collect()
}

fn working_days(p: &Project, first: Date, last: Date) -> Vec<Period> {
    let cal = schedule::project_calendar(p);
    let mut out = Vec::new();
    let mut day = first;
    for _ in 0..400 {
        if cal.is_working_day(day) {
            let d = u32::try_from(day.day()).unwrap_or(1);
            let m = u32::try_from(day.month()).unwrap_or(1);
            out.push(Period { start: day, end: day, label: format!("{d:02}-{m:02}") });
        }
        if day >= last {
            break;
        }
        day = dates::shift_days(day, 1);
    }
    out
}

fn weeks(first: Date, last: Date) -> Vec<Period> {
    let mut start = dates::shift_days(first, -i64::from(dates::weekday_index(first.weekday())));
    let mut out = Vec::new();
    for _ in 0..60 {
        let end = dates::shift_days(start, 6);
        let week = start.iso_week_date().week();
        out.push(Period { start, end, label: format!("W{week}") });
        if end >= last {
            break;
        }
        start = dates::shift_days(start, 7);
    }
    out
}

fn lighter(rgb: [u8; 3]) -> [u8; 3] {
    rgb.map(|c| ((u16::from(c) * 45 + 255 * 55) / 100) as u8)
}

/// Default bar colour for a task index (own colour, then ancestor, then chart style).
pub fn bar_color(p: &Project, idx: usize) -> [u8; 3] {
    if let Some(color) = p.tasks.get(idx).and_then(|t| t.bar_color) {
        return color;
    }
    let mut at = p.parent_index(idx);
    while let Some(parent) = at {
        if let Some(color) = p.tasks.get(parent).and_then(|t| t.bar_color) {
            return color;
        }
        at = p.parent_index(parent);
    }
    p.settings.chart_style.bar_rgb()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::Lag;
    use crate::model::LinkType;
    use crate::schedule::recalculate;
    use crate::xlsx_out::XlsxCell;

    fn sample_rows(p: &Project, idxs: &[usize]) -> (GanttIdentity, Vec<Option<GanttBar>>) {
        let headers = ["WBS", "Name", "Start", "Finish", "Duration", "Resource Names", "Predecessors", "Milestone", "Status"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut cells = Vec::new();
        let mut outlines = Vec::new();
        let mut bars = Vec::new();
        for &idx in idxs {
            let Some(task) = p.tasks.get(idx) else { continue };
            let summary = p.has_children(idx);
            let indent = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(8).min(8);
            let day = |d: Option<crate::dates::DateTime>| d.map(|d| crate::dates::dmy(d.date())).unwrap_or_default();
            let status = if summary {
                ""
            } else {
                match task.status {
                    crate::model::TaskStatus::NotStarted => "Not started",
                    crate::model::TaskStatus::Started => "Started",
                    crate::model::TaskStatus::Busy => "Busy",
                    crate::model::TaskStatus::Done => "Done",
                }
            };
            cells.push(vec![
                XlsxCell::plain(p.wbs(idx), summary, 0),
                XlsxCell::plain(task.name.clone(), summary, indent),
                XlsxCell::plain(day(task.sched.start), summary, 0),
                XlsxCell::plain(day(task.sched.finish), summary, 0),
                XlsxCell::plain(p.format_task_duration(task), summary, 0),
                XlsxCell::plain(p.format_resource_names_only(task.id), summary, 0),
                XlsxCell::plain(p.format_predecessors(task.id), summary, 0),
                XlsxCell::plain(if task.is_milestone() { "◆" } else { "" }, summary, 0),
                XlsxCell::plain(status, summary, 0),
            ]);
            outlines.push(indent.min(7));
            let span = task.sched.start.map(|s| {
                let start = s.date();
                let finish = task.sched.finish.map(|d| d.date()).unwrap_or(start);
                (start, finish)
            });
            bars.push(span.map(|(start, finish)| GanttBar {
                start,
                finish,
                fill: bar_color(p, idx),
                milestone: task.is_milestone(),
                summary,
            }));
        }
        (GanttIdentity { headers, cells, outlines }, bars)
    }

    #[test]
    fn two_tasks_fill_their_working_days() {
        let mut p = Project::new("Gantt", dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        let build = p.add_task("Build", None);
        p.set_duration(design, 480, false).unwrap();
        p.set_duration(build, 960, false).unwrap();
        p.link(design.into(), build, LinkType::FS, Lag::None).unwrap();
        let ana = p.add_resource("Ana");
        p.assign(design, ana, None, None).unwrap();
        recalculate(&mut p);
        let (id, bars) = sample_rows(&p, &[0, 1]);
        let bytes = workbook(&p, &id, &bars, None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("19-10") && text.contains("20-10") && text.contains("21-10"));
        assert!(text.contains("FF5A86D6"), "the default bar colour is a cell fill");
        for header in ["Start", "Finish", "Duration", "Resource Names", "Predecessors", "Milestone", "Status"] {
            assert!(text.contains(header), "missing header {header}");
        }
        assert!(text.contains("19-10-2026"), "Design starts Monday in dd-mm-yyyy");
        assert!(text.contains("Ana"), "the assigned resource is named");
        assert!(!text.contains("Ana["), "no inline unit percentages");
        assert!(text.contains(">1<") || text.contains("1FS"), "Build names its predecessor by row");
        assert!(text.contains("Not started"), "leaf tasks carry their status");
        assert!(text.contains(r#"xSplit="9""#), "the panes freeze after the identity columns");
        assert!(text.contains(r#"<c r="J2" s="18""#), "Design occupies Monday");
        assert!(text.contains(r#"<c r="K3" s="18""#) && text.contains(r#"<c r="L3" s="18""#), "Build occupies Tuesday and Wednesday");
        assert!(text.contains(r#"<c r="K2" s="0""#), "Design does not spill into Tuesday");
    }

    #[test]
    fn a_quarter_scale_gets_quarter_columns() {
        let mut p = Project::new("Gantt", dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        p.set_duration(design, 480, false).unwrap();
        recalculate(&mut p);
        let (id, bars) = sample_rows(&p, &[0]);
        let text = String::from_utf8_lossy(&workbook(&p, &id, &bars, Some(Granularity::Quarter)).unwrap()).to_string();
        assert!(text.contains("Q4 2026"), "one quarter column");
        assert!(!text.contains("19-10<"), "no day columns");
        let text = String::from_utf8_lossy(&workbook(&p, &id, &bars, Some(Granularity::Month)).unwrap()).to_string();
        assert!(text.contains("10-2026"), "one month column");
    }

    #[test]
    fn group_header_rows_get_a_summary_bar() {
        let mut p = Project::new("Gantt", dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        p.set_duration(design, 480, false).unwrap();
        recalculate(&mut p);
        let start = p.tasks[0].sched.start.unwrap().date();
        let finish = p.tasks[0].sched.finish.unwrap().date();
        let id = GanttIdentity {
            headers: vec!["Name".into()],
            cells: vec![vec![XlsxCell::plain("Status: To do (1)", true, 0)], vec![XlsxCell::plain("Design", false, 1)]],
            outlines: vec![0, 1],
        };
        let bars = vec![
            Some(GanttBar { start, finish, fill: [0x2D, 0x81, 0xFF], milestone: false, summary: true }),
            Some(GanttBar { start, finish, fill: bar_color(&p, 0), milestone: false, summary: false }),
        ];
        let text = String::from_utf8_lossy(&workbook(&p, &id, &bars, None).unwrap()).to_string();
        assert!(text.contains("Status: To do (1)"), "group header exports");
        assert!(text.contains("Design"));
        assert!(text.contains(r#"xSplit="1""#));
    }
}
