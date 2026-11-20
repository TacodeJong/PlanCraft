//! Excel Gantt of the rows the current filter leaves visible.

use plancraft_core::prelude::*;

use crate::state::UiState;

pub fn workbook(p: &Project, st: &UiState) -> Result<Vec<u8>, String> {
    let rows: Vec<usize> = super::outline_rows(p, st).into_iter().filter_map(|row| row.task()).collect();
    let words = plancraft_core::xlsx_gantt::GanttWords {
        wbs: st.t("col.wbs"),
        name: st.t("col.name"),
        start: st.t("col.start"),
        finish: st.t("col.finish"),
        duration: st.t("col.duration"),
        resources: st.t("col.resources"),
        predecessors: st.t("col.predecessors"),
        milestone: st.t("rpt.milestone"),
        status: st.t("col.status"),
        status_names: [
            crate::lang::status_name(TaskStatus::NotStarted),
            crate::lang::status_name(TaskStatus::Started),
            crate::lang::status_name(TaskStatus::Busy),
            crate::lang::status_name(TaskStatus::Done),
        ],
    };
    // The Q/M/D tier the chart showed carries into the export; zoomed to hours or
    // years, the automatic day/week choice decides.
    let scale = crate::state::Timescale::of_px(st.gantt.px_per_day).map(crate::state::Timescale::granularity);
    plancraft_core::xlsx_gantt::workbook(p, &rows, &words, scale)
}
