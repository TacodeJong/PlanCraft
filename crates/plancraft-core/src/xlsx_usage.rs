//! Task Usage as an Excel sheet: tasks with their assignments and time-phased work in
//! hours, one column per bucket of the chosen timescale. The name and work columns
//! freeze, so scrolling the periods keeps the rows identifiable.

use std::collections::BTreeMap;

use crate::dates::Date;
use crate::dates::prelude::*;
use crate::ids::AssignmentId;
use crate::model::Project;
use crate::workload::{Granularity, assignment_daily_work};
use crate::xlsx_out::{self, XlsxCell, XlsxRow, XlsxSheet};

/// The already-translated header words of the identity columns.
pub struct UsageWords<'a> {
    pub name: &'a str,
    pub work: &'a str,
}

/// The identity columns before the time-phased work; the panes freeze after them.
const IDENTITY_COLS: u32 = 2;

pub fn period_label(g: Granularity, d: Date) -> String {
    match g {
        Granularity::Day => format!("{:02}-{:02}", d.day32(), d.month32()),
        Granularity::Week => format!("W{}", d.iso_week_date().week()),
        Granularity::Month => format!("{:02}-{}", d.month32(), d.year32()),
        Granularity::Quarter => format!("Q{} {}", d.month0() / 3 + 1, d.year32()),
    }
}

/// Work in hours as Excel sees it: a number cell, blank when there is none.
fn hours_cell(mins: f64) -> XlsxCell {
    if mins < 0.5 {
        return XlsxCell::text("");
    }
    let h = mins / 60.0;
    if (h - h.round()).abs() < 0.005 { XlsxCell::number(format!("{}", h.round() as i64)) } else { XlsxCell::number(format!("{h:.2}")) }
}

/// The buckets spanning the scheduled dates of `rows`, and the work per assignment per
/// bucket in minutes. Shared by the workbook and the printed sheet.
pub fn phased(p: &Project, rows: &[usize], g: Granularity) -> (Vec<Date>, BTreeMap<AssignmentId, Vec<f64>>) {
    let mut first: Option<Date> = None;
    let mut last: Option<Date> = None;
    for &idx in rows {
        let Some(t) = p.tasks.get(idx) else { continue };
        let (Some(s), Some(f)) = (t.sched.start, t.sched.finish) else { continue };
        first = Some(first.map_or(s.date(), |d: Date| d.min(s.date())));
        last = Some(last.map_or(f.date(), |d: Date| d.max(f.date())));
    }
    let buckets = match (first, last) {
        (Some(f), Some(l)) => g.buckets(f, l),
        _ => Vec::new(),
    };
    let bucket_of = |d: Date| buckets.binary_search(&g.bucket_start(d)).ok();
    let mut per_assignment: BTreeMap<AssignmentId, Vec<f64>> = BTreeMap::new();
    for a in &p.assignments {
        let mut v = vec![0.0; buckets.len()];
        for (day, w) in assignment_daily_work(p, a) {
            if let Some(slot) = bucket_of(day).and_then(|b| v.get_mut(b)) {
                *slot += w;
            }
        }
        per_assignment.insert(a.id, v);
    }
    (buckets, per_assignment)
}

/// A task row's values: its own assignments, or the whole subtree for a summary.
pub fn task_values(p: &Project, i: usize, len: usize, per_assignment: &BTreeMap<AssignmentId, Vec<f64>>) -> Vec<f64> {
    let mut v = vec![0.0; len];
    let range = if p.has_children(i) { p.descendants_range(i) } else { i..i + 1 };
    for t in p.tasks.get(range).unwrap_or_default() {
        for a in p.assignments_for_task(t.id) {
            if let Some(av) = per_assignment.get(&a.id) {
                for (sum, x) in v.iter_mut().zip(av) {
                    *sum += x;
                }
            }
        }
    }
    v
}

/// `rows` is the outline order to export, ancestors included. Each task row is followed
/// by one row per assignment; summaries roll their subtrees up.
pub fn workbook(p: &Project, rows: &[usize], g: Granularity, words: &UsageWords<'_>) -> Result<Vec<u8>, String> {
    let (buckets, per_assignment) = phased(p, rows, g);
    let mut header: Vec<XlsxCell> = vec![XlsxCell::plain(words.name, true, 0), XlsxCell::plain(words.work, true, 0)];
    for b in &buckets {
        header.push(XlsxCell::plain(period_label(g, *b), true, 0));
    }
    let mut sheet_rows = vec![XlsxRow { cells: header, outline: 0 }];
    for &idx in rows {
        let Some(task) = p.tasks.get(idx) else { continue };
        let summary = p.has_children(idx);
        let indent = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(8).min(8);
        let mut cells = vec![
            XlsxCell::plain(task.name.clone(), summary, indent),
            XlsxCell::plain(crate::duration::format_work(task.sched.work), summary, 0),
        ];
        cells.extend(task_values(p, idx, buckets.len(), &per_assignment).into_iter().map(hours_cell));
        let outline = u8::try_from(task.outline_level.saturating_sub(1)).unwrap_or(7).min(7);
        sheet_rows.push(XlsxRow { cells, outline });
        for a in p.assignments_for_task(task.id) {
            let who = p.resource(a.resource).map(|r| r.name.clone()).unwrap_or_default();
            let mut cells = vec![
                XlsxCell::plain(who, false, indent.saturating_add(1).min(8)),
                XlsxCell::plain(crate::duration::format_work(a.work), false, 0),
            ];
            let values = per_assignment.get(&a.id).cloned().unwrap_or_default();
            cells.extend(values.into_iter().map(hours_cell));
            sheet_rows.push(XlsxRow { cells, outline: outline.saturating_add(1).min(7) });
        }
    }
    xlsx_out::sheets(&[XlsxSheet { name: "Task Usage".into(), rows: sheet_rows, table: false, freeze: (IDENTITY_COLS, 1) }])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::recalculate;

    /// A task row, its assignment row, period headers and the work values all land
    /// in the workbook, at the chosen timescale.
    #[test]
    fn tasks_their_assignments_and_buckets_fill_the_sheet() {
        let mut p = Project::new("Usage", crate::dates::ymd(2026, 10, 19).unwrap());
        let design = p.add_task("Design", None);
        p.set_duration(design, 960, false).unwrap();
        let ana = p.add_resource("Ana");
        p.assign(design, ana, None, None).unwrap();
        recalculate(&mut p);
        let words = UsageWords { name: "Task Name", work: "Work" };
        let text = String::from_utf8_lossy(&workbook(&p, &[0], Granularity::Day, &words).unwrap()).to_string();
        assert!(text.contains("Design") && text.contains("Ana"), "the task and its resource row");
        assert!(text.contains("19-10") && text.contains("20-10"), "day period headers");
        assert!(text.contains("16 hrs"), "the work total");
        assert!(text.contains(">8<"), "eight hours of work in a day bucket");
        assert!(text.contains(r#"xSplit="2""#), "the identity columns freeze");
        // The same plan in week buckets: one period, all sixteen hours in it.
        let text = String::from_utf8_lossy(&workbook(&p, &[0], Granularity::Week, &words).unwrap()).to_string();
        assert!(text.contains("W43"), "a week header");
        assert!(text.contains(">16<"), "the week rolls both days up");
    }
}
