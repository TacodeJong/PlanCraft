//! Send the open plan to Planner Premium. One button. Sign-in and the Tasks
//! tab happen in the browser; this window only says what is going on.

use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Context, RichText};

use plancraft_core::workspace::Document;

use crate::planner_send::{self, SendEvent, SendFail, SendStep};
use crate::theme;

pub struct PlannerSend {
    name: String,
    primed: bool,
    step: Option<SendStep>,
    project_id: String,
    url: String,
    schedule: String,
    tasks: Vec<String>,
    warnings: Vec<String>,
    error: Option<String>,
    done: bool,
    job: Option<Receiver<SendEvent>>,
}

impl Default for PlannerSend {
    fn default() -> Self {
        Self {
            name: String::new(),
            primed: false,
            step: None,
            project_id: String::new(),
            url: String::new(),
            schedule: String::new(),
            tasks: Vec::new(),
            warnings: Vec::new(),
            error: None,
            done: false,
            job: None,
        }
    }
}

impl PlannerSend {
    pub fn show(&mut self, ctx: &Context, doc: &Document, org: &str, toasts: &mut Vec<String>) -> bool {
        if !self.primed {
            self.name = doc.project.name.clone();
            self.primed = true;
        }
        self.poll(ctx, toasts);
        let mut open = true;
        let busy = self.job.is_some();
        egui::Window::new(t("send.title"))
            .id(egui::Id::unique("planner_send"))
            .title_bar(false)
            .default_width(480.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(440.0);
                if !crate::widgets::craft::dialog_header(ui, t("send.title")) {
                    open = false;
                }
                ui.label(t("send.explain"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(t("send.name"));
                    ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.name).desired_width(260.0));
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let label = if self.done { t("send.again") } else { t("send.button") };
                    if crate::widgets::craft::primary_button_enabled(ui, label, !busy).clicked() {
                        self.start(doc, org);
                    }
                    if busy {
                        ui.spinner();
                        if let Some(step) = self.step {
                            ui.label(step_text(step));
                        } else {
                            ui.label(t("send.working"));
                        }
                    }
                });
                if self.done && self.error.is_none() {
                    ui.add_space(8.0);
                    ui.label(RichText::new(t("send.done")).color(theme::pal().OK));
                }
                if !self.schedule.is_empty() {
                    ui.label(RichText::new(format!("{}: {}", t("send.dates"), self.schedule)).small());
                }
                if !self.tasks.is_empty() {
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        for task in &self.tasks {
                            ui.label(RichText::new(task).small());
                        }
                    });
                }
                for warning in self.warnings.iter().take(6) {
                    ui.label(RichText::new(warning_text(warning)).small().color(theme::pal().WARN));
                }
                if !self.url.is_empty() {
                    ui.add_space(4.0);
                    ui.hyperlink_to(t("send.open"), &self.url);
                }
                if !self.project_id.is_empty() {
                    ui.label(RichText::new(&self.project_id).small().color(theme::pal().TEXT_WEAK));
                }
                if let Some(err) = &self.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(err).color(theme::pal().ERROR));
                }
            });
        open
    }

    fn start(&mut self, doc: &Document, org: &str) {
        self.step = None;
        self.project_id.clear();
        self.url.clear();
        self.schedule.clear();
        self.tasks.clear();
        self.warnings.clear();
        self.error = None;
        self.done = false;
        let project = doc.project.clone();
        let name = planner_send::clean_name(&self.name);
        self.name = name.clone();
        self.job = Some(planner_send::start(project, name, org.to_string()));
    }

    fn poll(&mut self, ctx: &Context, toasts: &mut Vec<String>) {
        let Some(rx) = self.job.take() else { return };
        loop {
            match rx.try_recv() {
                Ok(SendEvent::Done(result)) => {
                    self.finish(result, toasts);
                    return;
                }
                Ok(event) => self.apply(event),
                Err(TryRecvError::Empty) => {
                    self.job = Some(rx);
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    if !self.done {
                        self.error = Some(t("send.start").into());
                    }
                    return;
                }
            }
        }
    }

    fn apply(&mut self, event: SendEvent) {
        match event {
            SendEvent::Step(step) => self.step = Some(step),
            SendEvent::Project(id) => {
                self.project_id = id;
                self.step = Some(SendStep::Created);
            }
            SendEvent::Link(url) => self.url = url,
            SendEvent::Schedule(text) => {
                self.schedule = text;
                self.step = Some(SendStep::Scheduling);
            }
            SendEvent::Task(text) => {
                if self.tasks.len() < 12 {
                    self.tasks.push(text);
                }
            }
            SendEvent::Warning(text) => {
                if self.warnings.len() < 8 {
                    self.warnings.push(text);
                }
            }
            SendEvent::Done(_) => {}
        }
    }

    fn finish(&mut self, result: Result<(), SendFail>, toasts: &mut Vec<String>) {
        self.job = None;
        match result {
            Ok(()) => {
                self.done = true;
                self.error = None;
                self.step = Some(SendStep::TasksOpen);
                toasts.push(t("send.done").into());
            }
            Err(fail) => {
                self.done = false;
                self.error = Some(fail_text(&fail));
            }
        }
    }
}

fn step_text(step: SendStep) -> &'static str {
    match step {
        SendStep::SignIn => t("send.sign_in"),
        SendStep::FileReady => t("send.file_ready"),
        SendStep::Created => t("send.created"),
        SendStep::Scheduling => t("send.scheduling"),
        SendStep::TasksOpen => t("send.tasks_open"),
    }
}

fn fail_text(fail: &SendFail) -> String {
    match fail {
        SendFail::NoTool => t("send.missing_tool").into(),
        SendFail::NoTemplate => t("send.missing_template").into(),
        SendFail::Write => t("send.write").into(),
        SendFail::Start => t("send.start").into(),
        SendFail::Rejected(message) => message.clone(),
    }
}

fn warning_text(warning: &str) -> String {
    if warning.contains("W_IMPORT_RESOURCE_NOTSUPPORTED") { t("send.resources").into() } else { warning.to_string() }
}

fn t(key: &'static str) -> &'static str {
    crate::lang::text(key)
}
