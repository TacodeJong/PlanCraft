//! PlanCraft desktop UI (egui). Craft L6.
//!
//! Frontends call [`run`]; the binary lives in `apps/plancraft`.

mod app;
#[cfg(target_os = "linux")]
mod bundled_libs;
mod columns;
mod commands;
mod craft_shell;
mod dialogs;
mod disk_watch;
mod distill;
mod edit;
mod engine_bridge;
mod fields;
mod fonts;
mod format;
mod icons;
mod lang;
mod logo;
#[cfg(target_os = "macos")]
mod mac_open;
mod mcp_host;
mod menu;
mod menubar;
mod open_requests;
mod panes;
mod planner_export;
mod planner_send;
mod printing;
mod quick_entry;
mod recovery;
mod remote;
mod settings;
mod start_page;
mod state;
mod theme;
mod views;
mod widgets;

use plancraft_core::workspace::Workspace;
use plancraft_engine::Session;

/// On Windows, a GUI-subsystem process has no console. When started from a terminal
/// for a command-line mode, attach to that terminal so output is visible.
#[cfg(windows)]
#[allow(unsafe_code)]
fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
    // SAFETY: plain integer Win32 calls; failure only means there is no console to attach to.
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        if out.is_null() || out == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

fn wgpu_options() -> eframe::WgpuConfiguration {
    #[allow(unused_mut)]
    let mut config = eframe::WgpuConfiguration::default();
    #[cfg(windows)]
    if std::env::var_os("WGPU_BACKEND").is_none()
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut config.wgpu_setup
    {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
    }
    config
}

/// Desktop entry: parse argv, optional headless subcommands, then open the window.
pub fn run() -> anyhow::Result<()> {
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let args: Vec<String> = raw.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    #[cfg(windows)]
    if matches!(args.first().map(String::as_str), Some("mcp" | "serve" | "--version" | "--write-icon")) {
        attach_parent_console();
    }
    if args.first().map(String::as_str) == Some("mcp") {
        let code = plancraft_mcp::cli::run(args.iter().skip(1).cloned().collect());
        std::process::exit(if code == std::process::ExitCode::SUCCESS { 0 } else { 1 });
    }
    if args.first().map(String::as_str) == Some("serve") {
        std::process::exit(plancraft_server::cli::run(args.iter().skip(1).cloned().collect()));
    }
    plancraft_core::lock::set_app_name(format!("PlanCraft {}", env!("CARGO_PKG_VERSION")));
    if args.first().map(String::as_str) == Some("--version") {
        println!("plancraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--write-icon") {
        let size: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(512).clamp(16, 2048);
        let img =
            image::RgbaImage::from_raw(size, size, logo::rasterize(size)).ok_or_else(|| anyhow::anyhow!("icon buffer size mismatch"))?;
        img.save(args.get(1).map(String::as_str).unwrap_or("plancraft-icon.png"))?;
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    bundled_libs::use_where_missing();

    let mut session = Session::new(Workspace::default());
    for a in &raw {
        if a.to_string_lossy().starts_with('-') {
            continue;
        }
        if plancraft_core::tabular::is_table_path(std::path::Path::new(a)) {
            open_requests::global().push([std::path::PathBuf::from(a)]);
            continue;
        }
        if let Err(e) = session.open_path(a) {
            eprintln!("plancraft: {e}");
        }
    }
    if args.iter().any(|a| a == "--demo") {
        let today = plancraft_core::dates::today();
        let back = i64::from(plancraft_core::dates::weekday_index(today.weekday())) + 14;
        let monday = plancraft_core::dates::add_days(today, -back).unwrap_or(today);
        session.add_project(plancraft_core::sample::demo_project(monday), None);
    }
    let shared = session.workspace.shared();
    #[cfg(target_os = "macos")]
    mac_open::install();

    // craft-window policy::apply — macOS Frame::Inset (content under traffic lights).
    let decision = craft_shell::decide_main_decision();
    let viewport = craft_shell::apply_frame(
        egui::ViewportBuilder::default()
            .with_title("PlanCraft")
            .with_app_id("plancraft")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_icon(logo::icon_data()),
        &decision.frame,
    );
    let options = eframe::NativeOptions { viewport, wgpu_options: wgpu_options(), ..Default::default() };
    let started = plancraft_core::guard::catch("opening the window", || {
        eframe::run_native("PlanCraft", options, Box::new(move |cc| Ok(Box::new(app::PlanCraftApp::new(cc, shared)))))
    });
    match started {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(no_window(&e.to_string())),
        Err(p) => Err(no_window(&p.message)),
    }
}

fn no_window(cause: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "PlanCraft's window could not be opened or failed: {cause}\n\
         PlanCraft needs a graphical desktop (X11 or Wayland) with working graphics drivers. If the message \
         above names a library, install it with your system's package manager. Without a desktop, \
         `plancraft mcp` runs the MCP server, which needs no window."
    )
}
