//! Provider, model and API key for Distill. The key goes to the system store, not settings.

use egui::{Context, RichText};

use crate::distill::{self, tail};
use crate::settings::{AiProvider, AppSettings};
use crate::theme;

#[derive(Default)]
pub struct AiState {
    paste: String,
    tail: Option<String>,
    loaded_for: Option<AiProvider>,
    changing: bool,
    error: Option<String>,
}

impl AiState {
    pub fn reload(&mut self) {
        self.loaded_for = None;
        self.paste.clear();
    }
}

pub fn show(ctx: &Context, open: &mut bool, state: &mut AiState, settings: &mut AppSettings) {
    let mut stay = *open;
    egui::Window::new(crate::lang::text("menu.ai"))
        .id(egui::Id::new("ai_settings"))
        .open(&mut stay)
        .default_width(460.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            let mut provider = settings.ai.provider;
            ui.horizontal(|ui| {
                ui.label(crate::lang::text("distill.service"));
                egui::ComboBox::from_id_salt("ai_provider").selected_text(provider.label()).show_ui(ui, |ui| {
                    for choice in AiProvider::ALL {
                        if ui.selectable_label(provider == choice, choice.label()).clicked() {
                            provider = choice;
                        }
                    }
                });
            });
            if provider != settings.ai.provider {
                settings.ai.provider = provider;
                settings.ai.model = provider.default_model().to_string();
                settings.save();
                state.loaded_for = None;
                state.paste.clear();
                state.changing = false;
            }
            ui.horizontal(|ui| {
                ui.label(crate::lang::text("distill.model"));
                if ui.add(egui::TextEdit::singleline(&mut settings.ai.model).desired_width(260.0)).changed() {
                    settings.save();
                }
            });
            if provider == AiProvider::Compatible {
                ui.horizontal(|ui| {
                    ui.label(crate::lang::text("distill.address"));
                    if ui.add(egui::TextEdit::singleline(&mut settings.ai.base_url).desired_width(260.0)).changed() {
                        settings.save();
                    }
                });
                ui.label(RichText::new(crate::lang::text("distill.address_hint")).small().color(theme::pal().TEXT_WEAK));
            }
            ui.add_space(8.0);
            if state.loaded_for != Some(provider) {
                match distill::load_key(provider.account()) {
                    Ok(Some(secret)) => {
                        state.tail = Some(tail(&secret));
                        state.changing = false;
                        state.error = None;
                    }
                    Ok(None) => {
                        state.tail = None;
                        state.changing = true;
                        state.error = None;
                    }
                    Err(e) => {
                        state.tail = None;
                        state.changing = true;
                        state.error = Some(e);
                    }
                }
                state.loaded_for = Some(provider);
            }
            if state.changing || state.tail.is_none() {
                ui.add(
                    egui::TextEdit::singleline(&mut state.paste)
                        .password(true)
                        .hint_text(crate::lang::text("distill.paste"))
                        .desired_width(280.0),
                );
                if ui.button(crate::lang::text("distill.save")).clicked() {
                    match distill::store_key(provider.account(), &state.paste) {
                        Ok(()) => {
                            state.tail = Some(tail(&state.paste));
                            state.paste.clear();
                            state.changing = false;
                            state.error = None;
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
            } else if let Some(tail) = &state.tail {
                ui.label(RichText::new(format!("{} · ••••{tail}", crate::lang::text("distill.saved"))).color(theme::pal().OK));
                ui.horizontal(|ui| {
                    if ui.button(crate::lang::text("distill.change")).clicked() {
                        state.changing = true;
                        state.paste.clear();
                    }
                    if ui.button(crate::lang::text("distill.remove")).clicked() {
                        match distill::delete_key(provider.account()) {
                            Ok(()) => {
                                state.tail = None;
                                state.changing = true;
                                state.paste.clear();
                                state.error = None;
                            }
                            Err(e) => state.error = Some(e),
                        }
                    }
                });
            }
            if let Some(err) = &state.error {
                ui.label(RichText::new(err).color(theme::pal().ERROR));
            }
            ui.label(RichText::new(crate::lang::text("distill.key_where")).small().color(theme::pal().TEXT_WEAK));
        });
    *open = stay;
}
