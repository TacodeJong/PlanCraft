//! Export a PlanCraft project to Microsoft Planner (Plan 1+ / basic board).
//!
//! Prefer **Sign in with Microsoft** via Azure CLI device login (no PlanCraft app id).
//! Optional: device-code with a public client under Advanced. Tokens stay in the keyring / `az`.

mod auth;
mod az_cli;
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

use std::sync::mpsc::Receiver;

use plancraft_core::prelude::Project;

use crate::settings::PlannerSettings;

use http::UreqTransport;

/// Azure CLI supplies the token when that was the sign-in, or when no app (client) id is configured.
fn prefer_azure_cli(az_mode: bool, client_id: &str) -> bool {
    az_mode || client_id.trim().is_empty()
}

fn auth_config(settings: &PlannerSettings) -> Result<AuthConfig, String> {
    let client = effective_client(&settings.client_id);
    if prefer_azure_cli(az_cli::using_az(), &client) {
        // Client/tenant unused when tokens come from `az`; a placeholder satisfies callers.
        return Ok(AuthConfig {
            tenant_id: effective_tenant(&settings.tenant_id),
            client_id: if client.is_empty() { az_cli::PLACEHOLDER_CLIENT_ID.to_string() } else { client },
        });
    }
    AuthConfig::from_settings(&effective_tenant(&settings.tenant_id), &client)
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
                "{} · {} tasks · {} deps · {} assignments · project {}",
                crate::lang::text("planner.done.premium"),
                r.tasks_created,
                r.dependencies_created,
                r.assignments_created,
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

/// Result of starting Sign in (Azure CLI device login, or Entra device-code).
pub enum SignInStart {
    /// Already have a usable `az` session.
    AzReady,
    /// Show device code; when `az_wait` is set, completion waits on Azure CLI instead of Entra poll.
    DeviceCode { pending: DeviceCode, az_wait: Option<Receiver<Result<(), String>>> },
}

/// Sign in with Microsoft: prefer Azure CLI (no PlanCraft app registration). Fallback: public client device-code.
pub fn sign_in(settings: &PlannerSettings) -> Result<SignInStart, String> {
    if az_cli::graph_token().is_ok() {
        az_cli::mark_az_signed_in()?;
        return Ok(SignInStart::AzReady);
    }
    // Azure CLI device login — uses Microsoft's az client; no tenant/client paste.
    match az_cli::begin_device_login() {
        Ok((pending, done)) => Ok(SignInStart::DeviceCode { pending, az_wait: Some(done) }),
        Err(az_err) => {
            // Optional fallback when a public client id is configured (Advanced / build default).
            if !effective_client(&settings.client_id).is_empty() {
                let cfg = AuthConfig::from_settings(&effective_tenant(&settings.tenant_id), &effective_client(&settings.client_id))?;
                let mut http = UreqTransport::default();
                let pending = begin_device_code(&mut http, &cfg, "https://graph.microsoft.com/.default offline_access openid profile")?;
                return Ok(SignInStart::DeviceCode { pending, az_wait: None });
            }
            Err(az_err)
        }
    }
}

pub fn complete_sign_in(settings: &PlannerSettings, pending: &DeviceCode) -> Result<(), String> {
    let cfg = auth_config(settings)?;
    let mut http = UreqTransport::default();
    let _ = poll_device_code(&mut http, &cfg, pending)?;
    let _ = az_cli::clear_az();
    Ok(())
}

pub fn complete_az_sign_in(done: &Receiver<Result<(), String>>) -> Result<(), String> {
    match done.recv_timeout(std::time::Duration::from_secs(900)) {
        Ok(Ok(())) => {
            // Confirm token works, then mark mode.
            let _ = az_cli::graph_token()?;
            az_cli::mark_az_signed_in()?;
            let _ = keys::delete_refresh_token();
            Ok(())
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(crate::lang::text("planner.auth_timeout").into()),
    }
}

pub fn signed_in() -> bool {
    if az_cli::using_az() {
        return az_cli::graph_token().is_ok();
    }
    keys::load_refresh_token().ok().flatten().is_some()
}

pub fn sign_out() -> Result<(), String> {
    let _ = az_cli::clear_az();
    keys::delete_refresh_token()
}

#[cfg(test)]
mod tests {
    use super::prefer_azure_cli;

    #[test]
    fn empty_client_id_uses_azure_cli() {
        assert!(prefer_azure_cli(false, ""));
        assert!(prefer_azure_cli(false, "   "));
        assert!(prefer_azure_cli(true, "00000000-0000-0000-0000-000000000000"));
        assert!(!prefer_azure_cli(false, "00000000-0000-0000-0000-000000000000"));
    }
}

pub fn load_groups(settings: &PlannerSettings) -> Result<Vec<(String, String)>, String> {
    let cfg = auth_config(settings)?;
    let mut http = UreqTransport::default();
    let tokens = graph_token(&mut http, &cfg)?;
    list_member_groups(&mut http, &tokens)
}
