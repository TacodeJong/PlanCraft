#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::fs;
use std::path::Path;

use egui::{Event, PointerButton, Rect, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use image::DynamicImage;

use super::*;

fn assert_rect_inside_window(h: &egui_kittest::Harness<'static, PlanCraftApp>, rect: Rect, what: &str) {
    let win = h.ctx.content_rect();
    assert!(
        rect.left() >= win.left() - 0.5
            && rect.right() <= win.right() + 0.5
            && rect.top() >= win.top() - 0.5
            && rect.bottom() <= win.bottom() + 0.5,
        "{what} must stay inside the content rect: {rect:?} in {win:?}"
    );
}

fn save_contact_sheet(out: &Path, name: &str, frames: &[(&'static str, DynamicImage)]) {
    assert!(!frames.is_empty(), "contact sheet needs frames");
    let frame_w = frames[0].1.width();
    let frame_h = frames[0].1.height();
    let cols = 2_u32;
    let rows = (frames.len() as u32).div_ceil(cols);
    let mut sheet = image::RgbaImage::new(frame_w * cols, frame_h * rows);
    for (i, (_, frame)) in frames.iter().enumerate() {
        let x = (i as u32 % cols) * frame_w;
        let y = (i as u32 / cols) * frame_h;
        image::imageops::overlay(&mut sheet, &frame.to_rgba8(), i64::from(x), i64::from(y));
    }
    DynamicImage::ImageRgba8(sheet).save(out.join(format!("{name}-contact-sheet.png"))).expect("save contact sheet");
    for (tag, frame) in frames {
        frame.save(out.join(format!("{name}-{tag}.png"))).unwrap_or_else(|e| panic!("save {name}-{tag}.png failed: {e}"));
    }
}

fn capture_theme_contact(tag: &str, lang: crate::lang::Lang, theme: crate::theme::ThemeChoice) -> Vec<(&'static str, DynamicImage)> {
    crate::lang::set_current(lang);
    let mut h = super::harness_at(1440.0);
    super::assert_harness_ready(&h);
    h.state_mut().st.lang = lang;
    let ctx = h.ctx.clone();
    h.state_mut().exec(&ctx, Command::SetTheme(theme));
    h.state_mut().st.menu_tab = crate::state::MenuTab::Task;
    h.run_steps(4);
    assert!(h.state().crash.is_none(), "{tag}: initial frame crashed: {:?}", h.state().crash);

    let mut frames: Vec<(&'static str, DynamicImage)> = Vec::new();

    // Primary action hover.
    let task_label = crate::lang::text("tb.task");
    let task_button = h
        .query_all_by_label(task_label)
        .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .max_by(|a, b| a.rect().center().y.partial_cmp(&b.rect().center().y).unwrap())
        .expect("toolbar +Task button");
    let task_center = task_button.rect().center();
    assert_rect_inside_window(&h, task_button.rect(), "primary toolbar button");
    h.event(Event::PointerMoved(task_center));
    h.run_steps(2);
    assert!(h.state().crash.is_none(), "{tag}: hover frame crashed: {:?}", h.state().crash);
    frames.push(("primary-hover", DynamicImage::ImageRgba8(h.render().expect("render primary hover"))));

    // Primary action down.
    h.event(Event::PointerButton { pos: task_center, button: PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE });
    h.run_steps(1);
    assert!(h.state().crash.is_none(), "{tag}: down frame crashed: {:?}", h.state().crash);
    frames.push(("primary-down", DynamicImage::ImageRgba8(h.render().expect("render primary down"))));
    h.event(Event::PointerButton { pos: task_center, button: PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE });
    h.run_steps(2);

    // Selected toggle hover (Critical Path in Format tab).
    h.state_mut().st.view = ViewKind::Gantt;
    h.state_mut().st.menu_tab = crate::state::MenuTab::Format;
    h.state_mut().st.gantt.show_critical = true;
    h.run_steps(3);
    let critical_label = crate::lang::text("fmt.critical");
    let critical_button = h
        .query_all_by_label(critical_label)
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("Critical Path button");
    assert_rect_inside_window(&h, critical_button.rect(), "selected toggle");
    h.event(Event::PointerMoved(critical_button.rect().center()));
    h.run_steps(2);
    assert!(h.state().crash.is_none(), "{tag}: selected-hover crash: {:?}", h.state().crash);
    frames.push(("selected-hover", DynamicImage::ImageRgba8(h.render().expect("render selected hover"))));

    // Open popup highlight (View tab More menu at narrow width).
    h.set_size(vec2(960.0, 900.0));
    h.state_mut().st.menu_tab = crate::state::MenuTab::View;
    h.run_steps(3);
    let more_label = crate::lang::text("tb.more");
    let more = h.get_by_label(more_label);
    assert_rect_inside_window(&h, more.rect(), "More launcher");
    more.click();
    h.run_steps(3);
    assert!(h.state().crash.is_none(), "{tag}: open-popup crash: {:?}", h.state().crash);
    h.get_by_label(lang.label());
    let popup_rect = h.ctx.data(|data| data.get_temp::<Rect>(egui::Id::unique("toolbar_more_popup_rect"))).expect("actual More popup body");
    assert!(popup_rect.width() > 0.0 && popup_rect.height() > 0.0);
    assert_rect_inside_window(&h, popup_rect, "More popup");
    frames.push(("popup-open", DynamicImage::ImageRgba8(h.render().expect("render popup open"))));

    // Representative real dialog/global text inheritance.
    egui::Popup::close_all(&h.ctx);
    h.state_mut().exec(&ctx, Command::Print);
    h.run_steps(4);
    assert!(h.state().dialogs.print.is_some(), "{tag}: print dialog should be open");
    assert!(h.query_all_by_role(egui::accesskit::Role::Window).next().is_some(), "{tag}: expected at least one rendered dialog window");
    frames.push(("dialog-global-fonts", DynamicImage::ImageRgba8(h.render().expect("render dialog state"))));

    frames
}

#[test]
#[ignore = "renders on the GPU; run explicitly for visual review"]
fn gui_consistency_preview_images() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target-toolbar/gui-consistency");
    fs::create_dir_all(&out).expect("create gui consistency output directory");
    let dark_en = capture_theme_contact("dark-en", crate::lang::Lang::En, crate::theme::ThemeChoice::Dark);
    save_contact_sheet(&out, "dark-en", &dark_en);
    let light_nl = capture_theme_contact("light-nl", crate::lang::Lang::Nl, crate::theme::ThemeChoice::Light);
    save_contact_sheet(&out, "light-nl", &light_nl);

    for file in [
        "dark-en-contact-sheet.png",
        "dark-en-primary-hover.png",
        "dark-en-primary-down.png",
        "dark-en-selected-hover.png",
        "dark-en-popup-open.png",
        "dark-en-dialog-global-fonts.png",
        "light-nl-contact-sheet.png",
        "light-nl-primary-hover.png",
        "light-nl-primary-down.png",
        "light-nl-selected-hover.png",
        "light-nl-popup-open.png",
        "light-nl-dialog-global-fonts.png",
    ] {
        assert!(out.join(file).exists(), "missing preview output: {file}");
    }
}
