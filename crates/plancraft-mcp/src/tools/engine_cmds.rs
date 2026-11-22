//! MCP tools that dispatch through `plancraft-engine::Session::execute`.
//!
//! Domain-specific tools remain for rich schemas; agents can also call the shared
//! command registry so UI, CLI and MCP stay on one path.

use serde_json::json;

use super::{Tool, schema};
use crate::args::{Args, ToolResult};
use crate::server::Ctx;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "list_commands",
            "List Commands",
            "Lists every engine command id (file.*, edit.*, task.*, …) shared with the CLI and desktop.",
            schema(json!({}), &[]),
            list_commands,
        )
        .read_only(),
        Tool::new(
            "run_command",
            "Run Command",
            "Runs one engine command by id with JSON params (same registry as plancraft-cli run).",
            schema(
                json!({
                    "id": { "type": "string", "description": "Command id, e.g. file.save, edit.undo, task.insert" },
                    "params": { "type": "object", "description": "JSON params for the command" },
                }),
                &["id"],
            ),
            run_command,
        ),
    ]
}

fn list_commands(ctx: &Ctx, a: &Args) -> ToolResult {
    ctx.run_engine_any(a, "commands.list", json!({}))
}

fn run_command(ctx: &Ctx, a: &Args) -> ToolResult {
    let id = a.req_str("id")?;
    let params = a.get("params").cloned().unwrap_or_else(|| json!({}));
    ctx.run_engine_any(a, id, params)
}
