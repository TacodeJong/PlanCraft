//! Excel Gantt of the rows the current filter leaves visible.

use plancraft_core::prelude::*;

use crate::state::UiState;

pub fn workbook(p: &Project, st: &UiState) -> Result<Vec<u8>, String> {
    let rows: Vec<usize> = super::outline_rows(p, st).into_iter().filter_map(|row| row.task()).collect();
    plancraft_core::xlsx_gantt::workbook(p, &rows, st.t("col.wbs"), st.t("col.name"))
}
