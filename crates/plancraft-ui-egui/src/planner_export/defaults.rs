//! Built-in Entra defaults for Planner export.
//!
//! The public client id is not a secret. When set, end users can Sign in without registering
//! an app. Override per machine via Advanced connection in the export dialog (local settings).

/// Authority tenant for multi-tenant device-code sign-in (any work/school org).
pub const DEFAULT_TENANT_ID: &str = "organizations";

/// PlanCraft multi-tenant public client application (client) id.
///
/// Empty until a maintainer registers **one** public multi-tenant Entra app (Allow public client
/// flows = Yes; delegated `Tasks.ReadWrite`, `Group.ReadWrite.All`, `User.Read`; plus Dataverse
/// `user_impersonation` for Plan 1+) and pastes the Application (client) ID here.
///
/// Override at build time: `PLANCRAFT_ENTRA_CLIENT_ID=<guid> cargo build …`
/// Or paste once under Advanced in the export dialog (stored only in local settings).
pub const DEFAULT_CLIENT_ID: &str = match option_env!("PLANCRAFT_ENTRA_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// True when PlanCraft ships a usable default public client id.
pub fn has_default_client() -> bool {
    !DEFAULT_CLIENT_ID.trim().is_empty()
}

/// Tenant used for auth: saved setting, else the multi-tenant default.
pub fn effective_tenant(stored: &str) -> String {
    let t = stored.trim();
    if t.is_empty() {
        DEFAULT_TENANT_ID.to_string()
    } else {
        t.to_string()
    }
}

/// Client id used for auth: saved setting, else the shipped public client.
pub fn effective_client(stored: &str) -> String {
    let c = stored.trim();
    if c.is_empty() {
        DEFAULT_CLIENT_ID.trim().to_string()
    } else {
        c.to_string()
    }
}
