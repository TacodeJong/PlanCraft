//! PlanCraft — project management in Rust.
//!
//! ```text
//! plancraft [FILE.plancraft]             open the desktop app
//! plancraft mcp [FILE...] [--http ADDR]  run the MCP server without a window
//! plancraft serve FOLDER [...]           share a folder of plans (PlanCraft Server)
//! ```

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> anyhow::Result<()> {
    plancraft_ui_egui::run()
}
