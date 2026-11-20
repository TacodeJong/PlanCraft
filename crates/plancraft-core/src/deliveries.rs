//! Who receives a task's output. Recipients are derived from links and assignments; nothing is stored.

use crate::dates::prelude::*;
use crate::ids::TaskId;
use crate::model::*;
use crate::report;

/// How far a delivery has got, as of the project's status moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryStatus {
    Waiting,
    InProgress,
    Late,
    Received,
}

impl DeliveryStatus {
    pub fn label(self) -> &'static str {
        match self {
            DeliveryStatus::Waiting => "Waiting",
            DeliveryStatus::InProgress => "In progress",
            DeliveryStatus::Late => "Late",
            DeliveryStatus::Received => "Received",
        }
    }
}

/// One incoming delivery for a recipient.
#[derive(Clone, Debug, PartialEq)]
pub struct Delivery {
    pub predecessor: TaskId,
    pub link: LinkType,
    pub via_group: Option<TaskId>,
    pub teams: Vec<String>,
    pub due: Option<DateTime>,
    pub progress: f32,
    pub status: DeliveryStatus,
}

/// A person or team that receives work, in outline order.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipient {
    pub name: String,
    pub deliveries: Vec<Delivery>,
}

/// Recipients and the deliveries they are waiting on.
pub fn recipients(p: &Project) -> Vec<Recipient> {
    let moment = report::status_moment(p);
    let mut out: Vec<Recipient> = Vec::new();
    for (leaf, task) in p.tasks.iter().enumerate() {
        if p.has_children(leaf) {
            continue;
        }
        let incoming = incoming(p, leaf);
        if incoming.is_empty() {
            continue;
        }
        for name in names_of(p, task.id) {
            let recipient = match out.iter_mut().find(|r| r.name == name) {
                Some(r) => r,
                None => {
                    out.push(Recipient { name: name.clone(), deliveries: Vec::new() });
                    match out.last_mut() {
                        Some(r) => r,
                        None => continue,
                    }
                }
            };
            for (pred, link, via) in &incoming {
                if recipient.deliveries.iter().any(|d| d.predecessor == *pred) {
                    continue;
                }
                recipient.deliveries.push(delivery(p, *pred, *link, *via, moment));
            }
        }
    }
    out
}

fn incoming(p: &Project, leaf: usize) -> Vec<(TaskId, LinkType, Option<TaskId>)> {
    let mut rows: Vec<(usize, TaskId, LinkType, Option<TaskId>)> = Vec::new();
    push_deps(p, leaf, None, &mut rows);
    let mut cursor = p.parent_index(leaf);
    while let Some(group) = cursor {
        if is_entry(p, leaf, group) {
            let via = p.tasks.get(group).map(|t| t.id);
            push_deps(p, group, via, &mut rows);
        }
        cursor = p.parent_index(group);
    }
    rows.sort_by_key(|row| row.0);
    rows.into_iter().map(|(_, id, link, via)| (id, link, via)).collect()
}

fn push_deps(p: &Project, idx: usize, via: Option<TaskId>, rows: &mut Vec<(usize, TaskId, LinkType, Option<TaskId>)>) {
    let Some(task) = p.tasks.get(idx) else { return };
    for dep in &task.predecessors {
        let Some(pred) = dep.predecessor.local() else { continue };
        let Some(pi) = p.task_index(pred) else { continue };
        if rows.iter().any(|row| row.1 == pred) {
            continue;
        }
        rows.push((pi, pred, dep.kind, via));
    }
}

/// A leaf is an entry of `group` when nothing inside the group (except the group itself) feeds it.
fn is_entry(p: &Project, leaf: usize, group: usize) -> bool {
    if leaf == group || !inside(p, leaf, group) {
        return false;
    }
    let mut cursor = Some(leaf);
    while let Some(idx) = cursor {
        if idx == group {
            return true;
        }
        let Some(task) = p.tasks.get(idx) else { return false };
        for dep in &task.predecessors {
            if let Some(pred) = dep.predecessor.local()
                && let Some(pi) = p.task_index(pred)
                && inside(p, pi, group)
                && pi != group
            {
                return false;
            }
        }
        cursor = p.parent_index(idx);
    }
    true
}

fn inside(p: &Project, idx: usize, group: usize) -> bool {
    idx == group || p.descendants_range(group).contains(&idx)
}

fn delivery(p: &Project, pred: TaskId, link: LinkType, via: Option<TaskId>, moment: DateTime) -> Delivery {
    let task = p.task(pred);
    let progress = task.map(progress_of).unwrap_or(0.0);
    let due = task.and_then(|t| t.sched.finish);
    let teams = task.map(|t| teams_of(p, t.id)).unwrap_or_else(|| vec!["Unassigned".into()]);
    let status = status_of(progress, due, moment);
    Delivery { predecessor: pred, link, via_group: via, teams, due, progress, status }
}

fn progress_of(t: &Task) -> f32 {
    if t.sched.is_summary { t.sched.percent_complete } else { f32::from(t.percent_complete) }
}

fn status_of(progress: f32, due: Option<DateTime>, moment: DateTime) -> DeliveryStatus {
    if progress >= 100.0 {
        DeliveryStatus::Received
    } else if due.is_some_and(|due| due < moment) {
        DeliveryStatus::Late
    } else if progress > 0.0 {
        DeliveryStatus::InProgress
    } else {
        DeliveryStatus::Waiting
    }
}

fn teams_of(p: &Project, id: TaskId) -> Vec<String> {
    let Some(idx) = p.task_index(id) else {
        return vec!["Unassigned".into()];
    };
    let mut names = Vec::new();
    let mut leaves = Vec::new();
    if p.has_children(idx) {
        for i in p.descendants_range(idx) {
            if !p.has_children(i) {
                leaves.push(i);
            }
        }
    } else {
        leaves.push(idx);
    }
    for i in leaves {
        let Some(task) = p.tasks.get(i) else { continue };
        for name in names_of(p, task.id) {
            if !names.iter().any(|n| n == &name) {
                names.push(name);
            }
        }
    }
    if names.is_empty() { vec!["Unassigned".into()] } else { names }
}

fn names_of(p: &Project, task: TaskId) -> Vec<String> {
    let mut names = Vec::new();
    for assignment in p.assignments.iter().filter(|a| a.task == task) {
        let Some(resource) = p.resources.iter().find(|r| r.id == assignment.resource) else { continue };
        if resource.name.is_empty() || names.iter().any(|n| n == &resource.name) {
            continue;
        }
        names.push(resource.name.clone());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::recalculate;

    #[test]
    fn ana_receives_a_finished_delivery_from_its_predecessor() {
        let mut p = Project::new("Deliveries", crate::dates::ymd(2026, 1, 5).unwrap());
        let a = p.add_task("A", None);
        let b = p.add_task("B", None);
        let ana = p.add_resource("Ana");
        p.task_mut(a).unwrap().percent_complete = 100;
        p.assign(b, ana, Some(1.0), None).unwrap();
        p.link(a.into(), b, LinkType::FS, crate::duration::Lag::None).unwrap();
        recalculate(&mut p);
        let list = recipients(&p);
        let ana = list.iter().find(|r| r.name == "Ana").expect("Ana is a recipient");
        let delivery = ana.deliveries.iter().find(|d| p.task(d.predecessor).is_some_and(|t| t.name == "A")).expect("A delivers to Ana");
        assert_eq!(delivery.status, DeliveryStatus::Received);
    }
}
