//! Export the open plan to Microsoft Planner (Plan 1+ PSS or basic Graph board).

use std::sync::mpsc::Receiver;
use std::thread;

use egui::{Context, RichText};

use plancraft_core::workspace::Document;

use crate::planner_export::{self, DeviceCode, ExportOutcome, ExportTarget};
use crate::settings::{AppSettings, PlannerSettings};
use crate::theme;

enum Job {
    Idle,
    SignIn(Receiver<Result<DeviceCode, String>>),
    CompleteSignIn(Receiver<Result<(), String>>),
    Groups(Receiver<Result<Vec<(String, String)>, String>>),
    Export(Receiver<Result<ExportOutcome, String>>),
}

pub struct PlannerExport {
    target: ExportTarget,
    group_id: String,
    groups: Vec<(String, String)>,
    secret_paste: String,
    device: Option<DeviceCode>,
    status: Option<String>,
    error: Option<String>,
    result: Option<String>,
    warnings: Vec<String>,
    signed_in: bool,
    job: Job,
}

impl Default for PlannerExport {
    fn default() -> Self {
        Self {
            target: ExportTarget::PremiumPss,
            group_id: String::new(),
            groups: Vec::new(),
            secret_paste: String::new(),
            device: None,
            status: None,
            error: None,
            result: None,
            warnings: Vec::new(),
            signed_in: planner_export::signed_in(),
            job: Job::Idle,
        }
    }
}

impl PlannerExport {
    pub fn show(&mut self, ctx: &Context, doc: &Document, settings: &mut AppSettings, toasts: &mut Vec<String>) -> bool {
        self.poll(ctx, settings, toasts);
        let mut open = true;
        egui::Window::new(t("planner.title"))
            .id(egui::Id::new("planner_export"))
            .title_bar(false)
            .default_width(520.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_min_width(480.0);
                if !crate::widgets::craft::dialog_header(ui, t("planner.title")) {
                    open = false;
                }
                ui.label(t("planner.explain"));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label(t("planner.tenant"));
                    if ui.add(egui::TextEdit::singleline(&mut settings.planner.tenant_id).desired_width(280.0)).changed() {
                        settings.save();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t("planner.client"));
                    if ui.add(egui::TextEdit::singleline(&mut settings.planner.client_id).desired_width(280.0)).changed() {
                        settings.save();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t("planner.org"));
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut settings.planner.dataverse_org_url)
                                .hint_text("https://org.crm4.dynamics.com")
                                .desired_width(280.0),
                        )
                        .changed()
                    {
                        settings.save();
                    }
                });
                ui.label(RichText::new(t("planner.org_hint")).small().color(theme::pal().TEXT_WEAK));

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(t("planner.secret"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.secret_paste)
                            .password(true)
                            .hint_text(t("planner.secret_hint"))
                            .desired_width(200.0),
                    );
                    if ui.button(t("distill.save")).clicked() && !self.secret_paste.trim().is_empty() {
                        match planner_export::store_client_secret(&self.secret_paste) {
                            Ok(()) => {
                                self.secret_paste.clear();
                                self.status = Some(t("planner.secret_saved").into());
                                self.error = None;
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                    if ui.button(t("distill.remove")).clicked() {
                        let _ = planner_export::delete_client_secret();
                        self.secret_paste.clear();
                    }
                });
                ui.label(RichText::new(t("planner.key_where")).small().color(theme::pal().TEXT_WEAK));

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(t("planner.target"));
                    egui::ComboBox::from_id_salt("planner_target").selected_text(self.target.label()).show_ui(ui, |ui| {
                        for choice in ExportTarget::ALL {
                            if ui.selectable_label(self.target == choice, choice.label()).clicked() {
                                self.target = choice;
                            }
                        }
                    });
                });
                if !self.target.includes_dependencies() {
                    ui.label(RichText::new(t("planner.note.no_deps")).color(theme::pal().WARN));
                }

                if self.target == ExportTarget::BasicBoard {
                    if self.group_id.is_empty() && !settings.planner.last_group_id.is_empty() {
                        self.group_id = settings.planner.last_group_id.clone();
                    }
                    ui.horizontal(|ui| {
                        ui.label(t("planner.group"));
                        egui::ComboBox::from_id_salt("planner_group").selected_text(self.group_label()).width(260.0).show_ui(ui, |ui| {
                            for (id, name) in &self.groups {
                                let label = if name.is_empty() { id.as_str() } else { name.as_str() };
                                if ui.selectable_label(self.group_id == *id, label).clicked() {
                                    self.group_id = id.clone();
                                    settings.planner.last_group_id = id.clone();
                                    settings.save();
                                }
                            }
                        });
                        if ui.add_enabled(self.signed_in && !self.busy(), egui::Button::new(t("planner.refresh_groups"))).clicked() {
                            self.start_groups(settings);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(t("planner.group_id"));
                        if ui.add(egui::TextEdit::singleline(&mut self.group_id).desired_width(280.0)).changed() {
                            settings.planner.last_group_id = self.group_id.clone();
                            settings.save();
                        }
                    });
                }

                ui.add_space(8.0);
                let preview = planner_export::build_export(&doc.project, self.target);
                ui.label(RichText::new(format!(
                    "{} · {} tasks · {} deps · {} people",
                    preview.title,
                    preview.tasks.len(),
                    preview.dependencies.len(),
                    preview.people.len()
                )));

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if self.signed_in {
                        ui.label(RichText::new(t("planner.signed_in")).color(theme::pal().OK));
                        if ui.add_enabled(!self.busy(), egui::Button::new(t("planner.sign_out"))).clicked() {
                            let _ = planner_export::sign_out();
                            self.signed_in = false;
                            self.device = None;
                        }
                    } else if ui.add_enabled(!self.busy(), egui::Button::new(t("planner.sign_in"))).clicked() {
                        self.start_sign_in(settings);
                    }
                    if ui
                        .add_enabled(self.signed_in && !self.busy() && self.can_export(settings), egui::Button::new(t("planner.export")))
                        .clicked()
                    {
                        self.start_export(doc, settings);
                    }
                    if self.busy() {
                        ui.spinner();
                        if let Some(status) = &self.status {
                            ui.label(status);
                        }
                    }
                });

                if let Some(device) = &self.device {
                    ui.separator();
                    ui.label(RichText::new(&device.message).strong());
                    ui.label(format!("{}: {}", t("planner.user_code"), device.user_code));
                    ui.hyperlink_to(&device.verification_uri, &device.verification_uri);
                    if ui.button(t("planner.open_login")).clicked() {
                        open_url(&device.verification_uri);
                    }
                }

                if let Some(err) = &self.error {
                    ui.label(RichText::new(err).color(theme::pal().ERROR));
                }
                if let Some(result) = &self.result {
                    ui.label(RichText::new(result).color(theme::pal().OK));
                }
                for w in &self.warnings {
                    ui.label(RichText::new(w).small().color(theme::pal().WARN));
                }
            });
        open
    }

    fn group_label(&self) -> String {
        self.groups
            .iter()
            .find(|(id, _)| id == &self.group_id)
            .map(|(_, name)| if name.is_empty() { self.group_id.clone() } else { name.clone() })
            .unwrap_or_else(|| if self.group_id.is_empty() { t("planner.pick_group").into() } else { self.group_id.clone() })
    }

    fn busy(&self) -> bool {
        !matches!(self.job, Job::Idle)
    }

    fn can_export(&self, settings: &AppSettings) -> bool {
        if settings.planner.tenant_id.trim().is_empty() || settings.planner.client_id.trim().is_empty() {
            return false;
        }
        match self.target {
            ExportTarget::PremiumPss => !settings.planner.dataverse_org_url.trim().is_empty(),
            ExportTarget::BasicBoard => !self.group_id.trim().is_empty(),
        }
    }

    fn start_sign_in(&mut self, settings: &AppSettings) {
        self.error = None;
        self.status = Some(t("planner.working_sign_in").into());
        let planner = settings.planner.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(planner_export::sign_in(&planner));
        });
        self.job = Job::SignIn(rx);
    }

    fn start_complete(&mut self, settings: &AppSettings, pending: DeviceCode) {
        self.status = Some(t("planner.working_wait").into());
        let planner = settings.planner.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(planner_export::complete_sign_in(&planner, &pending));
        });
        self.job = Job::CompleteSignIn(rx);
    }

    fn start_groups(&mut self, settings: &AppSettings) {
        self.error = None;
        self.status = Some(t("planner.working_groups").into());
        let planner = settings.planner.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(planner_export::load_groups(&planner));
        });
        self.job = Job::Groups(rx);
    }

    fn start_export(&mut self, doc: &Document, settings: &AppSettings) {
        self.error = None;
        self.result = None;
        self.warnings.clear();
        self.status = Some(t("planner.working_export").into());
        let planner = settings.planner.clone();
        let target = self.target;
        let group_id = self.group_id.clone();
        let project = doc.project.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(planner_export::export_project(&project, &planner, target, &group_id));
        });
        self.job = Job::Export(rx);
    }

    fn poll(&mut self, ctx: &Context, settings: &mut AppSettings, toasts: &mut Vec<String>) {
        let job = std::mem::replace(&mut self.job, Job::Idle);
        match job {
            Job::Idle => {}
            Job::SignIn(rx) => match rx.try_recv() {
                Ok(Ok(device)) => {
                    self.device = Some(device.clone());
                    self.status = None;
                    self.start_complete(settings, device);
                }
                Ok(Err(err)) => self.fail(err),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.job = Job::SignIn(rx);
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.fail(t("planner.offline").into()),
            },
            Job::CompleteSignIn(rx) => match rx.try_recv() {
                Ok(Ok(())) => {
                    self.signed_in = true;
                    self.device = None;
                    self.status = None;
                    toasts.push(t("planner.signed_in").into());
                }
                Ok(Err(err)) => self.fail(err),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.job = Job::CompleteSignIn(rx);
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.fail(t("planner.offline").into()),
            },
            Job::Groups(rx) => match rx.try_recv() {
                Ok(Ok(groups)) => {
                    self.groups = groups;
                    self.status = None;
                }
                Ok(Err(err)) => self.fail(err),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.job = Job::Groups(rx);
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.fail(t("planner.offline").into()),
            },
            Job::Export(rx) => match rx.try_recv() {
                Ok(Ok(outcome)) => {
                    self.status = None;
                    self.warnings = outcome.warnings().to_vec();
                    let summary = outcome.summary();
                    self.result = Some(summary.clone());
                    toasts.push(summary);
                }
                Ok(Err(err)) => self.fail(err),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.job = Job::Export(rx);
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.fail(t("planner.offline").into()),
            },
        }
    }

    fn fail(&mut self, err: String) {
        self.job = Job::Idle;
        self.status = None;
        self.error = Some(err);
    }
}

fn t(key: &'static str) -> &'static str {
    crate::lang::text(key)
}

fn open_url(url: &str) {
    let _ = (|| -> Result<(), ()> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(url).spawn().map_err(|_| ())?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn().map_err(|_| ())?;
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open").arg(url).spawn().map_err(|_| ())?;
        }
        Ok(())
    })();
}

#[allow(dead_code)]
fn _settings_type_check(s: &PlannerSettings) -> &str {
    s.tenant_id.as_str()
}
