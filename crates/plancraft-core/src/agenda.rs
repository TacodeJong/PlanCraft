//! Tasks due around the status moment: overdue, today, this week, and later milestones.

use crate::dates::{self, prelude::*};
use crate::ids::TaskId;
use crate::model::*;
use crate::report;
use crate::schedule::project_calendar;

/// Which list an agenda row belongs to. The lists do not overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgendaBucket {
    Overdue,
    Today,
    ThisWeek,
    Upcoming,
}

impl AgendaBucket {
    pub fn label(self) -> &'static str {
        match self {
            AgendaBucket::Overdue => "Overdue",
            AgendaBucket::Today => "Today",
            AgendaBucket::ThisWeek => "This week",
            AgendaBucket::Upcoming => "Upcoming milestones",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgendaItem {
    pub id: TaskId,
    pub name: String,
    pub finish: DateTime,
    pub percent: f32,
    pub milestone: bool,
    pub late: bool,
    pub row_mark: Option<RowMark>,
    pub bucket: AgendaBucket,
}

/// Non-summary tasks with a finish, placed in one bucket each.
pub fn items(p: &Project) -> Vec<AgendaItem> {
    let at = report::status_moment(p);
    let day = at.date();
    let week_end = dates::add_days(day, 7);
    let cal = project_calendar(p);
    let mut out = Vec::new();
    for (i, t) in p.tasks.iter().enumerate() {
        if p.has_children(i) {
            continue;
        }
        let Some(finish) = t.sched.finish else { continue };
        let complete = t.is_complete();
        let bucket = if !complete && finish < at {
            AgendaBucket::Overdue
        } else if finish.date() == day {
            AgendaBucket::Today
        } else if week_end.is_some_and(|end| finish.date() > day && finish.date() <= end) {
            AgendaBucket::ThisWeek
        } else if t.is_milestone() && finish >= at {
            AgendaBucket::Upcoming
        } else {
            continue;
        };
        let late = !complete && (finish < at || report::is_late_in(t, at, &cal));
        out.push(AgendaItem {
            id: t.id,
            name: t.name.clone(),
            finish,
            percent: f32::from(t.percent_complete),
            milestone: t.is_milestone(),
            late,
            row_mark: t.row_mark,
            bucket,
        });
    }
    out
}

/// Counts for the overview tiles. A task can be overdue and also due this week.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tiles {
    pub overdue: usize,
    pub this_week: usize,
    pub milestones: usize,
    pub waiting: usize,
}

/// One day of the fourteen-day strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayCount {
    pub date: Date,
    pub starting: usize,
    pub due: usize,
    pub milestones: usize,
}

fn named(p: &Project, id: TaskId, resource: Option<&str>) -> bool {
    let Some(name) = resource.map(str::trim).filter(|s| !s.is_empty()) else { return true };
    p.format_resource_names(id).split(',').any(|part| part.split('[').next().is_some_and(|who| who.trim().eq_ignore_ascii_case(name)))
}

/// Overdue, due this calendar week, milestones in the next 30 days, and deliveries still waiting.
pub fn tiles(p: &Project, resource: Option<&str>) -> Tiles {
    let at = report::status_moment(p);
    let day = at.date();
    let back = i64::from(crate::dates::weekday_index(day.weekday()));
    let monday = dates::shift_days(day, -back);
    let sunday = dates::shift_days(monday, 6);
    let horizon = dates::shift_days(day, 30);
    let mut tiles = Tiles::default();
    for (i, t) in p.tasks.iter().enumerate() {
        if p.has_children(i) || !named(p, t.id, resource) {
            continue;
        }
        let Some(finish) = t.sched.finish else { continue };
        let done = t.is_complete();
        if !done && finish < at {
            tiles.overdue = tiles.overdue.saturating_add(1);
        }
        let started = t.sched.start.map(|s| s.date());
        let due = finish.date();
        let in_week = (monday..=sunday).contains(&due) || started.is_some_and(|s| (monday..=sunday).contains(&s));
        if !done && in_week {
            tiles.this_week = tiles.this_week.saturating_add(1);
        }
        if t.is_milestone() && !done && due >= day && due <= horizon {
            tiles.milestones = tiles.milestones.saturating_add(1);
        }
    }
    tiles.waiting = crate::deliveries::recipients(p)
        .iter()
        .flat_map(|r| r.deliveries.iter())
        .filter(|d| d.status == crate::deliveries::DeliveryStatus::Waiting && named(p, d.predecessor, resource))
        .count();
    tiles
}

/// Fourteen days from the Monday of the week containing `today`.
pub fn strip(p: &Project, resource: Option<&str>, today: Date) -> Vec<DayCount> {
    let back = i64::from(crate::dates::weekday_index(today.weekday()));
    let mut day = dates::shift_days(today, -back);
    let mut out = Vec::with_capacity(14);
    for _ in 0..14 {
        let mut cell = DayCount { date: day, starting: 0, due: 0, milestones: 0 };
        for (i, t) in p.tasks.iter().enumerate() {
            if p.has_children(i) || !named(p, t.id, resource) {
                continue;
            }
            if t.sched.start.is_some_and(|s| s.date() == day) {
                cell.starting = cell.starting.saturating_add(1);
            }
            if t.is_milestone() {
                if t.sched.finish.is_some_and(|f| f.date() == day) {
                    cell.milestones = cell.milestones.saturating_add(1);
                }
            } else if t.sched.finish.is_some_and(|f| f.date() == day) {
                cell.due = cell.due.saturating_add(1);
            }
        }
        out.push(cell);
        day = dates::shift_days(day, 1);
    }
    out
}

/// A task that starts or finishes on one day of the strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayItem {
    pub id: TaskId,
    pub name: String,
    pub starting: bool,
    pub due: bool,
    pub milestone: bool,
}

/// Non-summary tasks that start or finish on `day`, in outline order.
pub fn on_day(p: &Project, resource: Option<&str>, day: Date) -> Vec<DayItem> {
    let mut out = Vec::new();
    for (i, t) in p.tasks.iter().enumerate() {
        if p.has_children(i) || !named(p, t.id, resource) {
            continue;
        }
        let starting = t.sched.start.is_some_and(|s| s.date() == day);
        let milestone = t.is_milestone() && t.sched.finish.is_some_and(|f| f.date() == day);
        let due = !t.is_milestone() && t.sched.finish.is_some_and(|f| f.date() == day);
        if starting || due || milestone {
            out.push(DayItem { id: t.id, name: t.name.clone(), starting, due, milestone });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::Lag;
    use crate::model::LinkType;
    use crate::schedule::recalculate;

    #[test]
    fn a_missed_finish_is_overdue() {
        let mut p = Project::new("Agenda", crate::dates::ymd(2026, 1, 5).unwrap());
        p.status_date = Some(crate::dates::ymd(2026, 3, 2).unwrap());
        let a = p.add_task("Late", None);
        p.task_mut(a).unwrap().duration = 480;
        recalculate(&mut p);
        let item = items(&p).into_iter().find(|item| item.name == "Late").expect("the task is on the agenda");
        assert_eq!(item.bucket, AgendaBucket::Overdue);
        assert!(item.late);
    }

    #[test]
    fn tiles_and_the_day_strip_count_the_same_tasks() {
        let mut p = Project::new("Agenda", crate::dates::ymd(2026, 10, 19).unwrap());
        p.status_date = Some(crate::dates::ymd(2026, 10, 21).unwrap());
        let late = p.add_task("Late", None);
        p.task_mut(late).unwrap().duration = 480;
        let soon = p.add_task("Soon", None);
        p.set_duration(soon, 480 * 3, false).unwrap();
        p.link(late.into(), soon, LinkType::FS, Lag::None).unwrap();
        let mark = p.add_milestone("Gate", None);
        p.link(soon.into(), mark, LinkType::FS, Lag::None).unwrap();
        recalculate(&mut p);
        let tiles = tiles(&p, None);
        assert!(tiles.overdue >= 1, "the one-day task finished before the status date");
        assert!(tiles.this_week >= 1);
        assert!(tiles.milestones >= 1);
        let soon_finish = p.task(soon).unwrap().sched.finish.unwrap().date();
        let day = strip(&p, None, crate::dates::ymd(2026, 10, 21).unwrap()).into_iter().find(|d| d.date == soon_finish);
        assert!(day.is_some_and(|d| d.due >= 1), "the strip counts the finish on its day");
    }
}
