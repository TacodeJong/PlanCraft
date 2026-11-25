//! Distill Plan: paste or open a brief, review the draft, then add it in one undo step.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

use egui::{Color32, Context, RichText, Sense, Stroke, Ui, vec2};

use crate::distill::{self, Draft, UreqChat};
use crate::settings::AiSettings;
use crate::theme;

struct Loaded {
    name: String,
    text: String,
    chars: usize,
    error: Option<String>,
}

enum Job {
    Idle,
    Running(Receiver<Result<Draft, String>>),
}

pub struct Distill {
    notes: String,
    files: Vec<Loaded>,
    error: Option<String>,
    draft: Option<Draft>,
    chain: bool,
    replace: bool,
    job: Job,
}

impl Default for Distill {
    fn default() -> Self {
        Self { notes: String::new(), files: Vec::new(), error: None, draft: None, chain: false, replace: false, job: Job::Idle }
    }
}

impl Distill {
    pub fn show(
        &mut self,
        ctx: &Context,
        ws: &mut plancraft_core::workspace::Workspace,
        handle: u64,
        ai: &AiSettings,
        open_ai: &mut bool,
        toasts: &mut Vec<String>,
    ) -> bool {
        self.poll(toasts);
        self.take_drops(ctx);
        let mut open = true;
        let mut close = false;
        let mut accept = false;
        {
            let Some(doc) = ws.index_of(handle).and_then(|i| ws.docs.get_mut(i)) else {
                return false;
            };
            egui::Window::new(t("distill.title"))
                .id(egui::Id::unique("distill_plan"))
                .title_bar(false)
                .default_width(760.0)
                .min_width(640.0)
                .show(ctx, |ui| {
                    ui.set_min_width(620.0);
                    open = crate::widgets::craft::dialog_header(ui, t("distill.title"));
                    ui.label(t("distill.explain"));
                    if crate::widgets::craft::secondary_button(ui, t("distill.ai")).clicked() {
                        *open_ai = true;
                    }
                    ui.add_space(6.0);
                    drop_zone(ui);
                    ui.horizontal(|ui| {
                        if crate::widgets::craft::secondary_button(ui, t("distill.choose")).clicked()
                            && let Some(path) = rfd::FileDialog::new().add_filter("Text", &["txt", "md", "markdown"]).pick_file()
                        {
                            self.files.push(load_file(&path));
                        }
                    });
                    self.files.retain(|file| file_chip(ui, file));
                    ui.add_space(4.0);
                    ui.add(
                        crate::widgets::craft::text_edit_multiline(&mut self.notes)
                            .desired_rows(6)
                            .hint_text(t("distill.notes"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);
                    let running = matches!(self.job, Job::Running(_));
                    ui.horizontal(|ui| {
                        if crate::widgets::craft::primary_button_enabled(
                            ui,
                            t("distill.analyze"),
                            !running && !self.source().trim().is_empty(),
                        )
                        .clicked()
                        {
                            self.start(ctx, ai, toasts);
                        }
                        if running {
                            ui.spinner();
                            ui.label(t("distill.working"));
                            if crate::widgets::craft::secondary_button(ui, t("distill.stop")).clicked() {
                                self.job = Job::Idle;
                            }
                        }
                    });
                    if let Some(err) = &self.error {
                        ui.label(RichText::new(err).color(theme::pal().ERROR));
                    }
                    if let Some(draft) = &self.draft {
                        ui.separator();
                        if !draft.summary.is_empty() {
                            ui.label(RichText::new(&draft.summary).strong());
                        }
                    }
                    let mut check = None;
                    ui.horizontal(|ui| {
                        if self.draft.is_some() && crate::widgets::craft::secondary_button(ui, t("distill.check_all")).clicked() {
                            check = Some(true);
                        }
                        if self.draft.is_some() && crate::widgets::craft::secondary_button(ui, t("distill.uncheck_all")).clicked() {
                            check = Some(false);
                        }
                    });
                    if let Some(on) = check
                        && let Some(draft) = &mut self.draft
                    {
                        for task in &mut draft.tasks {
                            task.included = on;
                        }
                    }
                    if self.draft.is_some() {
                        egui::ScrollArea::vertical().max_height(280.0).id_salt("distill_review").show(ui, |ui| {
                            review(ui, self.draft.as_mut());
                        });
                        ui.add_space(6.0);
                        let blank = doc.project.tasks.is_empty();
                        if blank {
                            self.replace = false;
                            ui.label(RichText::new(t("distill.add_to")).color(theme::pal().TEXT_WEAK));
                        } else {
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut self.replace, false, t("distill.add_to"));
                                ui.selectable_value(&mut self.replace, true, t("distill.replace"));
                            });
                        }
                        ui.checkbox(&mut self.chain, t("distill.chain"));
                        let n = self.draft.as_ref().map(|d| d.tasks.iter().filter(|task| task.included).count()).unwrap_or(0);
                        crate::widgets::craft::confirm_actions_rule(ui);
                        ui.horizontal(|ui| {
                            if crate::widgets::craft::secondary_button(ui, t("distill.cancel")).clicked() {
                                close = true;
                            }
                            let label = if self.replace { t("distill.replace") } else { t("distill.add") };
                            let label = format!("{label} ({n})");
                            if crate::widgets::craft::primary_button_enabled(ui, &label, n > 0).clicked() {
                                accept = true;
                            }
                        });
                    }
                });
        }
        if accept {
            match self.accept(ws, handle) {
                Ok(()) => close = true,
                Err(e) => self.fail(toasts, e),
            }
        }
        open && !close
    }

    fn fail(&mut self, toasts: &mut Vec<String>, message: impl Into<String>) {
        let message = message.into();
        toasts.push(message.clone());
        self.error = Some(message);
    }

    fn accept(&mut self, ws: &mut plancraft_core::workspace::Workspace, handle: u64) -> Result<(), String> {
        let Some(draft) = self.draft.clone() else {
            return Err(t("distill.none").into());
        };
        super::edit_bound(ws, handle, t("distill.undo"), |p| distill::apply_draft(p, &draft, self.replace, self.chain).map(|_| ()))?;
        Ok(())
    }

    fn source(&self) -> String {
        let mut parts: Vec<&str> = self.files.iter().filter(|f| f.error.is_none()).map(|f| f.text.as_str()).collect();
        if !self.notes.trim().is_empty() {
            parts.push(self.notes.trim());
        }
        parts.join("\n\n")
    }

    fn start(&mut self, ctx: &Context, ai: &AiSettings, toasts: &mut Vec<String>) {
        self.error = None;
        let source = self.source();
        let key = match distill::load_key(ai.provider.account()) {
            Ok(Some(key)) => key,
            Ok(None) => return self.fail(toasts, t("distill.need_key")),
            Err(e) => return self.fail(toasts, e),
        };
        let ai = ai.clone();
        let (tx, rx): (Sender<Result<Draft, String>>, _) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = distill::distill(&mut UreqChat, &ai, &key, &source);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.job = Job::Running(rx);
    }

    fn poll(&mut self, toasts: &mut Vec<String>) {
        let got = match &self.job {
            Job::Running(rx) => rx.try_recv(),
            Job::Idle => return,
        };
        match got {
            Ok(Ok(draft)) => {
                self.draft = Some(draft);
                self.error = None;
                self.job = Job::Idle;
            }
            Ok(Err(err)) => {
                self.job = Job::Idle;
                self.fail(toasts, format!("{} {err}", t("distill.bad")));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.job = Job::Idle;
                self.fail(toasts, t("distill.offline"));
            }
        }
    }

    fn take_drops(&mut self, ctx: &Context) {
        let paths: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|file| file.path().to_path_buf()).collect());
        for path in paths {
            self.files.push(load_file(&path));
        }
    }
}

fn t(key: &'static str) -> &'static str {
    crate::lang::text(key)
}

fn load_file(path: &Path) -> Loaded {
    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
    match read_brief(path) {
        Ok(text) => {
            let chars = text.chars().count();
            Loaded { name, text, chars, error: None }
        }
        Err(error) => Loaded { name, text: String::new(), chars: 0, error: Some(error) },
    }
}

fn read_brief(path: &Path) -> Result<String, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "markdown" | "text" => {}
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "xlsm" => return Err(t("distill.no_office").into()),
        _ => return Err(t("distill.need_text_file").into()),
    }
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > 1_000_000 {
        return Err(t("distill.too_large").into());
    }
    let text = std::fs::read_to_string(path).map_err(|_| t("distill.not_text").to_string())?;
    if text.starts_with("%PDF") {
        return Err(t("distill.no_office").into());
    }
    Ok(text)
}

fn file_chip(ui: &mut Ui, file: &Loaded) -> bool {
    let mut keep = true;
    ui.horizontal(|ui| {
        ui.label(&file.name);
        if let Some(err) = &file.error {
            ui.label(RichText::new(err).small().color(theme::pal().ERROR));
        } else {
            ui.label(RichText::new(format!("{}", file.chars)).small().color(theme::pal().TEXT_WEAK));
        }
        if ui.small_button("x").clicked() {
            keep = false;
        }
    });
    keep
}

fn drop_zone(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    dashed(ui.painter(), rect, theme::pal().GRID_LINE_STRONG);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        t("distill.drop"),
        egui::FontId::proportional(13.0),
        theme::pal().TEXT_WEAK,
    );
}

fn dashed(p: &egui::Painter, rect: egui::Rect, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    let dash = 6.0;
    let gap = 4.0;
    let mut x = rect.left();
    while x < rect.right() {
        let end = (x + dash).min(rect.right());
        p.hline(x..=end, rect.top(), stroke);
        p.hline(x..=end, rect.bottom(), stroke);
        x = end + gap;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        let end = (y + dash).min(rect.bottom());
        p.vline(rect.left(), y..=end, stroke);
        p.vline(rect.right(), y..=end, stroke);
        y = end + gap;
    }
}

fn review(ui: &mut Ui, draft: Option<&mut Draft>) {
    let Some(draft) = draft else { return };
    let groups: Vec<bool> = draft
        .tasks
        .iter()
        .enumerate()
        .map(|(i, task)| draft.tasks.get(i.saturating_add(1)).is_some_and(|next| next.level > task.level))
        .collect();
    for (task, group) in draft.tasks.iter_mut().zip(groups) {
        ui.horizontal(|ui| {
            ui.add_space(f32::from(task.level.saturating_sub(1)) * 16.0);
            ui.checkbox(&mut task.included, "");
            if task.milestone && !group {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                let c = rect.center();
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c + vec2(0.0, -5.0), c + vec2(5.0, 0.0), c + vec2(0.0, 5.0), c + vec2(-5.0, 0.0)],
                    theme::pal().MILESTONE,
                    Stroke::NONE,
                ));
            }
            ui.add(egui::TextEdit::singleline(&mut task.name).desired_width(280.0));
            let detail = if task.resources.is_empty() { String::new() } else { task.resources.join(", ") };
            if !detail.is_empty() {
                ui.label(RichText::new(detail).small().color(theme::pal().TEXT_WEAK));
            }
            let mark = if group {
                t("distill.group").to_string()
            } else if task.milestone {
                t("distill.milestone").to_string()
            } else if task.estimated {
                format!("{}{}?", task.days, t("distill.day"))
            } else {
                format!("{}{}", task.days, t("distill.day"))
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(mark);
            });
        });
    }
}
