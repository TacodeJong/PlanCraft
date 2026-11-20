//! Export a PlanCraft project to Microsoft Planner (Plan 1+ / basic board).
//!
//! Credentials (refresh token, optional client secret) live in the OS keyring.
//! Tenant / client defaults live in `defaults`; local settings can override.

mod auth;
mod defaults;
mod graph;
mod http;
mod keys;
mod map;
mod pss;

pub use auth::{AuthConfig, DeviceCode, begin_device_code, graph_token, poll_device_code};
pub use defaults::{DEFAULT_CLIENT_ID, DEFAULT_TENANT_ID, effective_client, effective_tenant, has_default_client};
pub use graph::{GraphExportResult, GraphOptions, export_basic, list_member_groups};
pub use keys::{delete_client_secret, store_client_secret};
pub use map::{ExportTarget, build_export};
pub use pss::{PssExportResult, PssOptions, export_premium};

use plancraft_core::prelude::Project;

use crate::settings::PlannerSettings;

use http::UreqTransport;

fn auth_config(settings: &PlannerSettings) -> Result<AuthConfig, String> {
    AuthConfig::from_settings(&effective_tenant(&settings.tenant_id), &effective_client(&settings.client_id))
}

#[derive(Clone, Debug)]
pub enum ExportOutcome {
    Basic(GraphExportResult),
    Premium(PssExportResult),
}

impl ExportOutcome {
    pub fn summary(&self) -> String {
        match self {
            ExportOutcome::Basic(r) => {
                format!("{} · {} tasks · plan {} · {}", crate::lang::text("planner.done.basic"), r.tasks_created, r.plan_id, r.plan_url)
            }
            ExportOutcome::Premium(r) => format!(
                "{} · {} tasks · {} deps · project {}",
                crate::lang::text("planner.done.premium"),
                r.tasks_created,
                r.dependencies_created,
                r.project_id
            ),
        }
    }

    pub fn warnings(&self) -> &[String] {
        match self {
            ExportOutcome::Basic(r) => &r.warnings,
            ExportOutcome::Premium(r) => &r.warnings,
        }
    }
}

/// Maps the open project and pushes it with the configured auth / target.
pub fn export_project(
    project: &Project,
    settings: &PlannerSettings,
    target: ExportTarget,
    group_id: &str,
) -> Result<ExportOutcome, String> {
    let cfg = auth_config(settings)?;
    let plan = build_export(project, target);
    let mut http = UreqTransport::default();
    match target {
        ExportTarget::BasicBoard => {
            let result = export_basic(&mut http, &cfg, &plan, &GraphOptions { group_id: group_id.to_string() })?;
            Ok(ExportOutcome::Basic(result))
        }
        ExportTarget::PremiumPss => {
            let result = export_premium(&mut http, &cfg, &plan, &PssOptions { org_url: settings.dataverse_org_url.clone() })?;
            Ok(ExportOutcome::Premium(result))
        }
    }
}

/// Device-code sign-in for Graph (stores refresh token in the keyring).
pub fn sign_in(settings: &PlannerSettings) -> Result<DeviceCode, String> {
    let cfg = auth_config(settings)?;
    let mut http = UreqTransport::default();
    begin_device_code(&mut http, &cfg, "https://graph.microsoft.com/.default offline_access openid profile")
}

pub fn complete_sign_in(settings: &PlannerSettings, pending: &DeviceCode) -> Result<(), String> {
    let cfg = auth_config(settings)?;
    let mut http = UreqTransport::default();
    let _ = poll_device_code(&mut http, &cfg, pending)?;
    Ok(())
}

pub fn signed_in() -> bool {
    keys::load_refresh_token().ok().flatten().is_some()
}

pub fn sign_out() -> Result<(), String> {
    keys::delete_refresh_token()
}

pub fn load_groups(settings: &PlannerSettings) -> Result<Vec<(String, String)>, String> {
    let cfg = auth_config(settings)?;
    let mut http = UreqTransport::default();
    let tokens = graph_token(&mut http, &cfg)?;
    list_member_groups(&mut http, &tokens)
}

/// True when Sign in can start (shipped default client and/or local override).
pub fn can_sign_in(settings: &PlannerSettings) -> bool {
    !effective_client(&settings.client_id).is_empty() && !effective_tenant(&settings.tenant_id).is_empty()
}
