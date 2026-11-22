//! Project-level mutation commands (`project.*` writers; inspect stays in mod).

use plancraft_core::model::ChartStyle;
use serde_json::{Value, json};

use super::params::opt_str;
use crate::session::{Session, ok_empty};

pub fn run_update_progress_from_time(s: &mut Session, _p: &Value) -> Result<Value, String> {
    s.edit_active("Update Progress from Time", |proj| {
        proj.update_progress_from_time();
    })?;
    Ok(ok_empty())
}

pub fn run_set_chart_style(s: &mut Session, p: &Value) -> Result<Value, String> {
    let style = match opt_str(p, "style")?.unwrap_or("blue") {
        "blue" => ChartStyle::Blue,
        "graphite" => ChartStyle::Graphite,
        "green" => ChartStyle::Green,
        "orange" => ChartStyle::Orange,
        "high_contrast" | "high-contrast" => ChartStyle::HighContrast,
        other => return Err(format!("unknown chart style '{other}' (blue|graphite|green|orange|high_contrast)")),
    };
    s.edit_active("Design style", |proj| {
        proj.settings.chart_style = style;
    })?;
    Ok(json!({ "style": match style {
        ChartStyle::Blue => "blue",
        ChartStyle::Graphite => "graphite",
        ChartStyle::Green => "green",
        ChartStyle::Orange => "orange",
        ChartStyle::HighContrast => "high_contrast",
    }}))
}
