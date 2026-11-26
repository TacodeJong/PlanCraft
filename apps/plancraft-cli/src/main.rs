//! Headless PlanCraft: list/run engine commands, or start MCP.
//!
//! ```text
//! plancraft-cli commands
//! plancraft-cli run <id> [--params JSON] [FILE...]
//! plancraft-cli inspect [FILE...]
//! plancraft-cli export-xml [--planner] <plan> [output.xml]
//! plancraft-cli mcp [FILE...] [--http ADDR]
//! ```

use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use plancraft_core::workspace::{ChangeSource, Workspace};
use plancraft_engine::{Session, command};
use serde_json::{Value, json};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plancraft-cli: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "commands" => {
            let list = command::registry();
            for c in list {
                println!("{}\t{}\t{}", c.id, c.label, c.params);
            }
            Ok(())
        }
        "run" => {
            args.remove(0);
            let id = args.first().context("usage: plancraft-cli run <id> [--params JSON] [FILE...]")?.clone();
            args.remove(0);
            let mut params = json!({});
            let mut files = Vec::new();
            let mut i = 0;
            while i < args.len() {
                let arg = args.get(i).context("argument index")?;
                if arg == "--params" {
                    let raw = args.get(i + 1).context("--params needs a JSON value")?;
                    params = serde_json::from_str(raw).context("invalid --params JSON")?;
                    i += 2;
                } else {
                    files.push(arg.clone());
                    i += 1;
                }
            }
            let mut session = session_with_files(&files)?;
            let out = session.execute(&id, &params).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        "inspect" => {
            args.remove(0);
            let mut session = session_with_files(&args)?;
            let out = session.execute("project.inspect", &Value::Object(Default::default())).map_err(|e| anyhow::anyhow!("{e}"))?;
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(())
        }
        "export-xml" => {
            args.remove(0);
            export_xml(&args)
        }
        "mcp" => {
            args.remove(0);
            let code = plancraft_mcp::cli::run(args);
            if code == ExitCode::SUCCESS { Ok(()) } else { bail!("mcp exited with failure") }
        }
        other => bail!("unknown subcommand '{other}' (try help)"),
    }
}

fn export_xml(args: &[String]) -> Result<()> {
    let mut planner = false;
    let mut files = Vec::new();
    for arg in args {
        if arg == "--planner" {
            planner = true;
        } else {
            files.push(arg.clone());
        }
    }
    let input = files.first().context("usage: plancraft-cli export-xml [--planner] <plan> [output.xml]")?;
    let output = match files.get(1) {
        Some(path) => path.clone(),
        None => {
            let mut path = std::path::PathBuf::from(input);
            path.set_extension("xml");
            path.to_string_lossy().into_owned()
        }
    };
    let project = plancraft_core::io::load(input).map_err(|e| anyhow::anyhow!("{e}"))?;
    let notes = plancraft_core::mspdi::save_with(&project, &output, planner).map_err(|e| anyhow::anyhow!("{e}"))?;
    for note in notes {
        eprintln!("{note}");
    }
    println!("{output}");
    Ok(())
}

fn session_with_files(files: &[String]) -> Result<Session> {
    let mut session = Session::new(Workspace::default()).with_source(ChangeSource::User);
    for f in files {
        session.open_path(f).map_err(|e| anyhow::anyhow!("opening {f}: {e}"))?;
    }
    Ok(session)
}

fn print_help() {
    eprintln!(
        "\
PlanCraft CLI

USAGE:
    plancraft-cli commands
    plancraft-cli run <id> [--params JSON] [FILE...]
    plancraft-cli inspect [FILE...]
    plancraft-cli export-xml [--planner] <plan> [output.xml]
    plancraft-cli mcp [FILE...] [--http ADDR]
"
    );
}
