//! A Gantt chart as an Excel sheet: one row per task and one filled cell per
//! period the task covers. The caller may pass the timescale; without one,
//! days when the plan fits in about three months, weeks when it does not.

use crate::dates::prelude::*;
use crate::dates::{self, Date};
use crate::model::{Project, Task};
use crate::schedule;
use crate::workload::Granularity;
use crate::xlsx_out::{self, XlsxCell, XlsxRow, XlsxSheet};

struct Period {
    start: Date,
    end: Date,
    label: String,
}

/// The already-translated header words of the identity columns, and the status names
/// (one per [`crate::model::TaskStatus::ALL`] entry, in that order).
pub struct GanttWords<'a> {
    pub wbs: &'a str,
    pub name: &'a str,
    pub start: &'a str,
    pub finish: &'a str,
    pub duration: &'a str,
    pub resources: &'a str,
    pub predecessors: &'a str,
    pub milestone: &'a str,
    pub status: &'a str,
    pub status_names: [&'a str; 4],
}

/// The identity columns before the time-phased fills; the panes freeze after them,
/// so scrolling the calendar keeps dates, resources and predecessors visible.
const IDENTITY_COLS: u32 = 9;

/// `rows` is the outline order to export, ancestors included. `scale` is the Q/M/D
/// timescale the view showed; `None` keeps the automatic day/week choice.
pub fn workbook(p: &Project, rows: &[usize], words: &GanttWords<'_>, scale: Option<Granularity>) -> Result<Vec<u8>, String> {
    let periods = periods(p, rows, scale);
    let mut sheet_rows = Vec::with_capacity(rows.len().saturating_add(1));
    let mut header: Vec<XlsxCell> = [
        words.wbs,
        words.name,
        words.start,
        words.finish,
        words.duration,
        words.resources,
        words.predecessors,
        words.milestone,
        words.status,
    ]
    .into_iter()
    .map(|w| XlsxCell::plain(w, true, 0))
    .collect();
    for period in &periods {
        header.push(XlsxCell::plain(&period.label, true, 0));
    }
    sheet_rows.push(XlsxRow { cells: header, outline: 0 });
    for &idx in rows {
        let Some(task) = p.tasks.get(idx) else { continue };
        let summary = p.has_children(idx);
        let indent = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(8).min(8);
        let day = |d: Option<crate::dates::DateTime>| d.map(|d| crate::dates::dmy(d.date())).unwrap_or_default();
        // A summary's status rolls up from its subtasks: left to the schedule columns there.
        let status = if summary {
            ""
        } else {
            crate::model::TaskStatus::ALL
                .iter()
                .position(|s| *s == task.status)
                .and_then(|i| words.status_names.get(i).copied())
                .unwrap_or("")
        };
        let mut cells = vec![
            XlsxCell::plain(p.wbs(idx), summary, 0),
            XlsxCell::plain(task.name.clone(), summary, indent),
            XlsxCell::plain(day(task.sched.start), summary, 0),
            XlsxCell::plain(day(task.sched.finish), summary, 0),
            XlsxCell::plain(p.format_task_duration(task), summary, 0),
            XlsxCell::plain(p.format_resource_names_only(task.id), summary, 0),
            XlsxCell::plain(p.format_predecessors(task.id), summary, 0),
            XlsxCell::plain(if task.is_milestone() { "◆" } else { "" }, summary, 0),
            XlsxCell::plain(status, summary, 0),
        ];
        let span = task_span(task);
        let color = bar_color(p, idx);
        let fill = if summary { lighter(color) } else { color };
        for period in &periods {
            cells.push(period_cell(task, span, period, fill, summary));
        }
        let outline = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(7).min(7);
        sheet_rows.push(XlsxRow { cells, outline });
    }
    xlsx_out::sheets(&[XlsxSheet { name: "Gantt".into(), rows: sheet_rows, table: false, freeze: (IDENTITY_COLS, 1) }])
}

fn period_cell(task: &Task, span: Option<(Date, Date)>, period: &Period, fill: [u8; 3], summary: bool) -> XlsxCell {
    let Some((start, finish)) = span else {
        return XlsxCell::text("");
    };
    if start > period.end || finish < period.start {
        return XlsxCell::text("");
    }
    if task.is_milestone() {
        let mark = if start >= period.start && start <= period.end { "◆" } else { "" };
        return XlsxCell::filled(mark, false, fill);
    }
    XlsxCell::filled("", summary, fill)
}

fn periods(p: &Project, rows: &[usize], scale: Option<Granularity>) -> Vec<Period> {
    let mut first = None;
    let mut last = None;
    for &idx in rows {
        let Some(span) = p.tasks.get(idx).and_then(task_span) else { continue };
        first = Some(first.map_or(span.0, |d: Date| d.min(span.0)));
        last = Some(last.map_or(span.1, |d: Date| d.max(span.1)));
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

/// Month or quarter columns, labelled "11-2026" and "Q4 2026".
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

fn task_span(task: &Task) -> Option<(Date, Date)> {
    let start = task.sched.start?.date();
    let finish = task.sched.finish.map(|d| d.date()).unwrap_or(start);
    Some((start, finish))
}

fn bar_color(p: &Project, idx: usize) -> [u8; 3] {
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

fn lighter(rgb: [u8; 3]) -> [u8; 3] {
    rgb.map(|c| ((u16::from(c) * 45 + 255 * 55) / 100) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::Lag;
    use crate::model::LinkType;
    use crate::schedule::recalculate;

    fn words() -> GanttWords<'static> {
        GanttWords {
            wbs: "WBS",
            name: "Name",
            start: "Start",
            finish: "Finish",
            duration: "Duration",
            resources: "Resource Names",
            predecessors: "Predecessors",
            milestone: "Milestone",
            status: "Status",
            status_names: ["Not started", "Started", "Busy", "Done"],
        }
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
        let bytes = workbook(&p, &[0, 1], &words(), None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("19-10") && text.contains("20-10") && text.contains("21-10"));
        assert!(text.contains("FF5A86D6"), "the default bar colour is a cell fill");
        // The identity columns sit before the periods: dates, resources and predecessors.
        for header in ["Start", "Finish", "Duration", "Resource Names", "Predecessors", "Milestone", "Status"] {
            assert!(text.contains(header), "missing header {header}");
        }
        assert!(text.contains("19-10-2026"), "Design starts Monday in dd-mm-yyyy");
        assert!(text.contains("Ana"), "the assigned resource is named");
        assert!(!text.contains("Ana["), "no inline unit percentages");
        assert!(text.contains(">1<") || text.contains("1FS"), "Build names its predecessor by row");
        assert!(text.contains("Not started"), "leaf tasks carry their status");
        assert!(text.contains(r#"xSplit="9""#), "the panes freeze after the identity columns");
        // The bar fills moved right by the seven new columns: C..E became J..L.
        assert!(text.contains(r#"<c r="J2" s="18""#), "Design occupies Monday");
        assert!(text.contains(r#"<c r="K3" s="18""#) && text.contains(r#"<c r="L3" s="18""#), "Build occupies Tuesday and Wednesday");
        assert!(text.contains(r#"<c r="K2" s="0""#), "Design does not spill into Tuesday");
    }

    /// The Q/M/D selector carries into the export: a quarter scale gets quarter columns
    /// even for a plan the automatic choice would print in days.
    #[test]
    fn a_quarter_scale_gets_quarter_columns() {
        let mut p = Project::new("Gantt", dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        p.set_duration(design, 480, false).unwrap();
        recalculate(&mut p);
        let text = String::from_utf8_lossy(&workbook(&p, &[0], &words(), Some(Granularity::Quarter)).unwrap()).to_string();
        assert!(text.contains("Q4 2026"), "one quarter column");
        assert!(!text.contains("19-10<"), "no day columns");
        let text = String::from_utf8_lossy(&workbook(&p, &[0], &words(), Some(Granularity::Month)).unwrap()).to_string();
        assert!(text.contains("10-2026"), "one month column");
    }
}
