//! The Task Board: tasks as cards in status columns (Microsoft Project's Task Board view).
//!
//! Microsoft's published description of the board:
//! - Each task's Board Status names its column.
//! - New tasks start in Not Started.
//! - Columns can be renamed, and new ones are added to the right of the last.
//! - Moving a card to another column changes its Board Status.
//! - A column can carry a "Set % Complete" value that a task moved into it takes.
//! - Show on Board (Yes by default) hides a task from the board.
//! - A project has at most 100 columns.
//!
//! Choices where the pages say nothing:
//! - A new plan's columns are Not Started, Next up, In progress and Done (the pages name Not
//!   Started for new tasks and the other three for a sprint board), and Done sets 100%.
//! - A task with no Board Status (or one naming a deleted column) sits in the first column.
//! - Summary tasks don't show; their subtasks do.
//! - Deleting a column moves its tasks to the first column.

use serde::{Deserialize, Serialize};

use crate::ids::{BoardColumnId, IdCounters, TaskId};
use crate::limits::MAX_NAME_CHARS;
use crate::model::{Project, TaskStatus};
use crate::ops::{PlanError, PlanResult};

/// Most columns a board can have.
pub const MAX_BOARD_COLUMNS: usize = 100;

/// A Task Board column (a Board Status value).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardColumn {
    pub id: BoardColumnId,
    pub name: String,
    /// Set % Complete: a task moved into this column takes this % complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<u8>,
    /// The workflow status this column mirrors. A linked column and the Status field are one:
    /// a card moved here takes the status (with its percent rules), and a task's card sits in
    /// its status's column unless an unlinked (custom) column holds it explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
}

/// The four canonical columns and the status each one mirrors, by stored (file) name.
/// The UI shows the localized status names (Niet gestart, Gestart, Bezig, Gereed).
pub const CANONICAL: [(&str, TaskStatus); 4] = [
    ("Not Started", TaskStatus::NotStarted),
    ("Next up", TaskStatus::Started),
    ("In progress", TaskStatus::Busy),
    ("Done", TaskStatus::Done),
];

/// The board a new plan starts with: the four status columns (Done sets 100% complete).
pub fn default_columns(ids: &mut IdCounters) -> Vec<BoardColumn> {
    CANONICAL
        .into_iter()
        .map(|(name, status)| BoardColumn {
            id: ids.next_board_column(),
            name: name.into(),
            percent_complete: (status == TaskStatus::Done).then_some(100),
            status: Some(status),
        })
        .collect()
}

/// Migration for plans saved before columns carried a status: the canonical columns (by name,
/// case ignored) link to their statuses, each status at most once. Renamed or custom columns
/// are left alone.
pub fn link_status_columns(p: &mut Project) {
    for (name, status) in CANONICAL {
        if p.board_columns.iter().any(|c| c.status == Some(status)) {
            continue;
        }
        let key = name_key(name);
        if let Some(c) = p.board_columns.iter_mut().find(|c| c.status.is_none() && name_key(&c.name) == key) {
            c.status = Some(status);
        }
    }
}

fn invalid<T>(msg: impl Into<String>) -> PlanResult<T> {
    Err(PlanError::Invalid(msg.into()))
}

/// What makes two column names the same: case and surrounding spaces don't count. Used by the
/// ops and by the file checks alike, so they always agree.
pub fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A column id written as "B3" or "3" (digits only).
fn parse_id(key: &str) -> Option<BoardColumnId> {
    let digits = key.strip_prefix(['B', 'b']).unwrap_or(key);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(BoardColumnId)
}

/// A trimmed column name, or why it can't be one.
fn column_name(name: &str) -> PlanResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return invalid("a column needs a name");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return invalid(format!("a column name can be at most {MAX_NAME_CHARS} characters"));
    }
    Ok(name.to_string())
}

impl Project {
    pub fn board_column(&self, id: BoardColumnId) -> Option<&crate::board::BoardColumn> {
        self.board_columns.iter().find(|c| c.id == id)
    }

    /// A column by name (case ignored) or by id ("B3" or "3"); a name wins.
    pub fn find_board_column(&self, key: &str) -> Option<BoardColumnId> {
        let k = name_key(key);
        if let Some(c) = self.board_columns.iter().find(|c| name_key(&c.name) == k) {
            return Some(c.id);
        }
        self.board_column_by_id(key.trim())
    }

    /// A column by id only ("B3" or "3").
    pub fn board_column_by_id(&self, key: &str) -> Option<BoardColumnId> {
        parse_id(key).and_then(|id| self.board_column(id)).map(|c| c.id)
    }

    /// The column linked to a status, if any.
    pub fn status_column(&self, status: TaskStatus) -> Option<BoardColumnId> {
        self.board_columns.iter().find(|c| c.status == Some(status)).map(|c| c.id)
    }

    /// The column a task's card sits in. An explicit Board Status naming a custom (unlinked)
    /// column wins; otherwise the task's workflow status places the card in its linked column,
    /// so Status and board stay one. Fallback: the first column.
    pub fn board_column_of(&self, idx: usize) -> Option<BoardColumnId> {
        let t = self.tasks.get(idx)?;
        let custom = t.board_status.filter(|s| self.board_column(*s).is_some_and(|c| c.status.is_none()));
        custom.or_else(|| self.status_column(t.status)).or_else(|| self.board_columns.first().map(|c| c.id))
    }

    /// Whether the task at `idx` has a card: shown on the board and not a summary task.
    pub fn on_board(&self, idx: usize) -> bool {
        self.tasks.get(idx).is_some_and(|t| t.show_on_board) && !self.has_children(idx)
    }

    /// The board: each column with the rows of its cards, in outline order.
    pub fn board(&self) -> Vec<(&crate::board::BoardColumn, Vec<usize>)> {
        let mut cols: Vec<(&crate::board::BoardColumn, Vec<usize>)> = self.board_columns.iter().map(|c| (c, Vec::new())).collect();
        let at: std::collections::HashMap<BoardColumnId, usize> = self.board_columns.iter().enumerate().map(|(k, c)| (c.id, k)).collect();
        for i in 0..self.tasks.len() {
            if !self.on_board(i) {
                continue;
            }
            let k = self.board_column_of(i).and_then(|id| at.get(&id).copied()).unwrap_or(0);
            if let Some((_, rows)) = cols.get_mut(k) {
                rows.push(i);
            }
        }
        cols
    }

    /// Rows of `ids`, looked up in one pass (an unknown id is an error).
    fn rows_of(&self, ids: &[TaskId]) -> PlanResult<Vec<usize>> {
        let at: std::collections::HashMap<TaskId, usize> = self.tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        ids.iter().map(|id| at.get(id).copied().ok_or(PlanError::TaskNotFound(*id))).collect()
    }

    fn require_column(&self, id: BoardColumnId) -> PlanResult<usize> {
        self.board_columns.iter().position(|c| c.id == id).ok_or_else(|| PlanError::Invalid(format!("there is no board column {id}")))
    }

    /// Adds a column at position `at` (to the right of the last when `None`).
    pub fn add_board_column(&mut self, name: &str, at: Option<usize>, percent_complete: Option<u8>) -> PlanResult<BoardColumnId> {
        let name = column_name(name)?;
        if self.board_columns.len() >= MAX_BOARD_COLUMNS {
            return invalid(format!("a board can have at most {MAX_BOARD_COLUMNS} columns"));
        }
        if self.board_columns.iter().any(|c| name_key(&c.name) == name_key(&name)) {
            return invalid(format!("there is already a column named '{name}'"));
        }
        if percent_complete.is_some_and(|p| p > 100) {
            return invalid("% complete is at most 100");
        }
        let id = self.ids.next_board_column();
        let at = at.unwrap_or(self.board_columns.len()).min(self.board_columns.len());
        self.board_columns.insert(at, BoardColumn { id, name, percent_complete, status: None });
        Ok(id)
    }

    pub fn rename_board_column(&mut self, id: BoardColumnId, name: &str) -> PlanResult<()> {
        let k = self.require_column(id)?;
        let name = column_name(name)?;
        if self.board_columns.iter().any(|c| c.id != id && name_key(&c.name) == name_key(&name)) {
            return invalid(format!("there is already a column named '{name}'"));
        }
        if let Some(c) = self.board_columns.get_mut(k) {
            c.name = name;
        }
        Ok(())
    }

    /// Sets (or with `None` clears) the % complete a task moved into the column takes.
    pub fn set_board_column_percent(&mut self, id: BoardColumnId, percent_complete: Option<u8>) -> PlanResult<()> {
        let k = self.require_column(id)?;
        if percent_complete.is_some_and(|p| p > 100) {
            return invalid("% complete is at most 100");
        }
        if let Some(c) = self.board_columns.get_mut(k) {
            c.percent_complete = percent_complete;
        }
        Ok(())
    }

    /// Moves a column to position `to` (clamped).
    pub fn move_board_column(&mut self, id: BoardColumnId, to: usize) -> PlanResult<()> {
        let k = self.require_column(id)?;
        let col = self.board_columns.remove(k);
        let to = to.min(self.board_columns.len());
        self.board_columns.insert(to, col);
        Ok(())
    }

    /// Deletes a column; its tasks move to the first remaining column. The last column can't be
    /// deleted. Returns the number of cards moved.
    pub fn delete_board_column(&mut self, id: BoardColumnId) -> PlanResult<usize> {
        let k = self.require_column(id)?;
        if self.board_columns.len() <= 1 {
            return invalid("the board needs at least one column");
        }
        let moved = self.board().get(k).map_or(0, |(_, rows)| rows.len());
        self.board_columns.remove(k);
        for t in &mut self.tasks {
            if t.board_status == Some(id) {
                t.board_status = None;
            }
        }
        Ok(moved)
    }

    /// Moves tasks' cards to a column (their Board Status). A status-linked column sets the
    /// tasks' workflow status, with its percent rules; a column with a Set % Complete value
    /// gives them that % complete. Summary tasks are refused (their subtasks have the cards).
    pub fn set_board_status(&mut self, ids: &[TaskId], column: BoardColumnId) -> PlanResult<()> {
        let k = self.require_column(column)?;
        let col = self.board_columns.get(k).cloned();
        let (percent, status) = col.map(|c| (c.percent_complete, c.status)).unwrap_or_default();
        let rows = self.rows_of(ids)?;
        if let Some(&i) = rows.iter().find(|&&i| self.has_children(i)) {
            let name = self.tasks.get(i).map(|t| t.name.clone()).unwrap_or_default();
            return invalid(format!("'{name}' is a summary task; its subtasks have the cards"));
        }
        for i in rows {
            if let Some(t) = self.tasks.get_mut(i) {
                // In a linked column the status is the position; the explicit Board Status is
                // only for custom columns.
                t.board_status = if status.is_some() { None } else { Some(column) };
            }
            if let Some(s) = status
                && let Some(id) = self.tasks.get(i).map(|t| t.id)
            {
                self.set_task_status(id, s)?;
            }
            if let Some(p) = percent {
                self.set_percent_complete_at(i, p)?;
            }
        }
        Ok(())
    }

    /// Shows or hides tasks on the board (Show on Board).
    pub fn set_show_on_board(&mut self, ids: &[TaskId], show: bool) -> PlanResult<()> {
        for i in self.rows_of(ids)? {
            if let Some(t) = self.tasks.get_mut(i) {
                t.show_on_board = show;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use crate::prelude::*;

    fn plan() -> (Project, TaskId, TaskId) {
        let mut p = Project::new("B", crate::dates::ymd(2026, 10, 19).unwrap());
        let a = p.add_task("A", None);
        let b = p.add_task("B", None);
        crate::schedule::recalculate(&mut p);
        (p, a, b)
    }

    #[test]
    fn columns_add_rename_move_and_delete() {
        let (mut p, a, _) = plan();
        let names = |p: &Project| p.board_columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&p), ["Not Started", "Next up", "In progress", "Done"]);
        let review = p.add_board_column("Review", None, None).unwrap();
        assert!(p.add_board_column(" review ", None, None).is_err(), "names are unique");
        assert!(p.add_board_column("", None, None).is_err());
        p.move_board_column(review, 3).unwrap();
        assert_eq!(names(&p)[3], "Review");
        p.rename_board_column(review, "QA").unwrap();
        assert_eq!(p.find_board_column("qa"), Some(review));
        assert_eq!(p.find_board_column(&review.to_string()), Some(review));
        p.set_board_status(&[a], review).unwrap();
        assert_eq!(p.delete_board_column(review).unwrap(), 1);
        assert_eq!(p.task(a).unwrap().board_status, None);
        assert_eq!(p.board_column_of(p.task_index(a).unwrap()), p.board_columns.first().map(|c| c.id));
        while p.board_columns.len() > 1 {
            let id = p.board_columns[0].id;
            p.delete_board_column(id).unwrap();
        }
        assert!(p.delete_board_column(p.board_columns[0].id).is_err(), "one column stays");
        for k in 0..99 {
            p.add_board_column(&format!("C{k}"), None, None).unwrap();
        }
        assert!(p.add_board_column("One too many", None, None).is_err());
    }

    /// Regressions: the ops compared names with ASCII-only case folding while the file check
    /// lowercased fully, so some names passed one and were renamed by the other (the edit rolled
    /// back); loose ids like "BB3" or "+3" resolved; deleting the first column under-counted the
    /// cards that moved (tasks with no Board Status already sat there).
    #[test]
    fn names_ids_and_counts_are_consistent() {
        let (mut p, a, b) = plan();
        p.add_board_column("ÉTÉ", None, None).unwrap();
        assert!(p.add_board_column("été", None, None).is_err());
        assert!(crate::limits::sanitize(&mut p.clone()).is_empty());
        let third = p.board_columns[2].id;
        for loose in ["BB3", "+3", "B", "b+3", " 3x"] {
            assert_eq!(p.find_board_column(loose), None, "{loose}");
        }
        assert_eq!(p.find_board_column("b3"), Some(third));
        let named = p.add_board_column("3", None, None).unwrap();
        assert_eq!(p.find_board_column("3"), Some(named), "a name wins over an id");
        assert_eq!(p.board_column_by_id("3"), Some(third));
        // A and B sit in the first column with no Board Status; one more was put there.
        let first = p.board_columns[0].id;
        p.set_board_status(&[b], first).unwrap();
        assert_eq!(p.delete_board_column(first).unwrap(), 2);
        assert!(p.task(a).unwrap().board_status.is_none() && p.task(b).unwrap().board_status.is_none());
    }

    #[test]
    fn summaries_have_no_card_and_hidden_tasks_leave_the_board() {
        let (mut p, a, b) = plan();
        p.indent(b).unwrap();
        let done = p.board_columns[3].id;
        assert!(p.set_board_status(&[a], done).is_err(), "A is now a summary");
        p.set_board_status(&[b], done).unwrap();
        assert_eq!(p.task(b).unwrap().percent_complete, 100, "Done sets 100%");
        let board = p.board();
        assert!(board.iter().all(|(_, rows)| !rows.contains(&0)));
        assert_eq!(board[3].1, [1]);
        p.set_show_on_board(&[b], false).unwrap();
        assert!(p.board().iter().all(|(_, rows)| rows.is_empty()));
    }

    /// Status ↔ board sync: moving a card into a linked column IS a status edit, with the
    /// status's percent rules.
    #[test]
    fn a_card_moved_into_a_linked_column_takes_the_status() {
        let (mut p, a, _) = plan();
        let busy = p.status_column(TaskStatus::Busy).unwrap();
        p.set_board_status(&[a], busy).unwrap();
        let t = p.task(a).unwrap();
        assert_eq!(t.status, TaskStatus::Busy);
        assert_eq!(t.board_status, None, "a linked column needs no explicit Board Status");
        let done = p.status_column(TaskStatus::Done).unwrap();
        p.set_board_status(&[a], done).unwrap();
        let t = p.task(a).unwrap();
        assert_eq!((t.status, t.percent_complete), (TaskStatus::Done, 100));
        let not_started = p.status_column(TaskStatus::NotStarted).unwrap();
        p.set_board_status(&[a], not_started).unwrap();
        let t = p.task(a).unwrap();
        assert_eq!((t.status, t.percent_complete), (TaskStatus::NotStarted, 0), "back to not started clears the percent");
    }

    /// Status ↔ board sync, the other direction: a status edit moves the card.
    #[test]
    fn a_status_edit_moves_the_card() {
        let (mut p, a, b) = plan();
        p.set_task_status(a, TaskStatus::Busy).unwrap();
        let board = p.board();
        assert_eq!(board[2].1, [p.task_index(a).unwrap()], "A's card sits under Busy");
        assert_eq!(board[0].1, [p.task_index(b).unwrap()], "B never left Not Started");
        p.set_task_status(a, TaskStatus::Done).unwrap();
        assert_eq!(p.board()[3].1, [p.task_index(a).unwrap()]);
    }

    /// A custom (unlinked) column holds its cards explicitly and never touches the status.
    #[test]
    fn a_custom_column_keeps_its_card_and_leaves_the_status_alone() {
        let (mut p, a, _) = plan();
        p.set_task_status(a, TaskStatus::Busy).unwrap();
        let review = p.add_board_column("Review", None, None).unwrap();
        p.set_board_status(&[a], review).unwrap();
        let t = p.task(a).unwrap();
        assert_eq!(t.status, TaskStatus::Busy, "a custom column is not a status");
        assert_eq!(t.board_status, Some(review));
        assert_eq!(p.board()[4].1, [p.task_index(a).unwrap()]);
        // Deleting the custom column drops the explicit placement: the status places the card.
        p.delete_board_column(review).unwrap();
        assert_eq!(p.board()[2].1, [p.task_index(a).unwrap()], "back under Busy, not the first column");
    }

    /// Migration: canonical columns from plans saved before columns carried a status link up by
    /// name, each status once; renamed or custom columns stay unlinked.
    #[test]
    fn an_old_files_canonical_columns_link_to_the_statuses() {
        let (mut p, _, _) = plan();
        for c in &mut p.board_columns {
            c.status = None; // as read from an old file
        }
        p.add_board_column("Review", None, None).unwrap();
        crate::board::link_status_columns(&mut p);
        let statuses: Vec<Option<TaskStatus>> = p.board_columns.iter().map(|c| c.status).collect();
        assert_eq!(
            statuses,
            [Some(TaskStatus::NotStarted), Some(TaskStatus::Started), Some(TaskStatus::Busy), Some(TaskStatus::Done), None]
        );
        // A second run changes nothing, and a renamed canonical column stays unlinked.
        let busy = p.status_column(TaskStatus::Busy).unwrap();
        p.rename_board_column(busy, "Working").unwrap();
        if let Some(c) = p.board_columns.iter_mut().find(|c| c.id == busy) {
            c.status = None;
        }
        crate::board::link_status_columns(&mut p);
        assert_eq!(p.status_column(TaskStatus::Busy), None, "the renamed column is not re-linked");
    }
}
