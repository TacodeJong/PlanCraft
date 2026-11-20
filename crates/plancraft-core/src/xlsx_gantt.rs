//! A Gantt chart as an Excel sheet: one row per task and one filled cell per
//! period the task covers. Days when the plan fits in about three months,
//! weeks when it does not.

use crate::dates::{self, Date};
use crate::model::{Project, Task};
use crate::schedule;
use crate::xlsx_out::{self, XlsxCell, XlsxRow, XlsxSheet};

struct Period {
    start: Date,
    end: Date,
    label: String,
}

/// `rows` is the outline order to export, ancestors included. `wbs` and `name`
/// are the already-translated header words.
pub fn workbook(p: &Project, rows: &[usize], wbs: &str, name: &str) -> Result<Vec<u8>, String> {
    let periods = periods(p, rows);
    let mut sheet_rows = Vec::with_capacity(rows.len().saturating_add(1));
    let mut header = vec![XlsxCell::plain(wbs, true, 0), XlsxCell::plain(name, true, 0)];
    for period in &periods {
        header.push(XlsxCell::plain(&period.label, true, 0));
    }
    sheet_rows.push(XlsxRow { cells: header, outline: 0 });
    for &idx in rows {
        let Some(task) = p.tasks.get(idx) else { continue };
        let summary = p.has_children(idx);
        let indent = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(8).min(8);
        let mut cells = vec![XlsxCell::plain(p.wbs(idx), summary, 0), XlsxCell::plain(task.name.clone(), summary, indent)];
        let span = task_span(task);
        let color = bar_color(p, idx);
        let fill = if summary { lighter(color) } else { color };
        for period in &periods {
            cells.push(period_cell(task, span, period, fill, summary));
        }
        let outline = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(7).min(7);
        sheet_rows.push(XlsxRow { cells, outline });
    }
    xlsx_out::sheets(&[XlsxSheet { name: "Gantt".into(), rows: sheet_rows, table: false, freeze: (2, 1) }])
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

fn periods(p: &Project, rows: &[usize]) -> Vec<Period> {
    let mut first = None;
    let mut last = None;
    for &idx in rows {
        let Some(span) = p.tasks.get(idx).and_then(task_span) else { continue };
        first = Some(first.map_or(span.0, |d: Date| d.min(span.0)));
        last = Some(last.map_or(span.1, |d: Date| d.max(span.1)));
    }
    let (Some(first), Some(last)) = (first, last) else { return Vec::new() };
    if dates::days_between(first, last) > 90 { weeks(first, last) } else { working_days(p, first, last) }
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

    #[test]
    fn two_tasks_fill_their_working_days() {
        let mut p = Project::new("Gantt", dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        let build = p.add_task("Build", None);
        p.set_duration(design, 480, false).unwrap();
        p.set_duration(build, 960, false).unwrap();
        p.link(design.into(), build, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let bytes = workbook(&p, &[0, 1], "WBS", "Name").unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("19-10") && text.contains("20-10") && text.contains("21-10"));
        assert!(text.contains("FF5A86D6"), "the default bar colour is a cell fill");
        assert!(text.contains(r#"<c r="C2" s="18""#), "Design occupies Monday");
        assert!(text.contains(r#"<c r="D3" s="18""#) && text.contains(r#"<c r="E3" s="18""#), "Build occupies Tuesday and Wednesday");
        assert!(text.contains(r#"<c r="D2" s="0""#), "Design does not spill into Tuesday");
    }
}
