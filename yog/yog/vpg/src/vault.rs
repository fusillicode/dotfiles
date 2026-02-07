use std::process::Command;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use serde::Deserialize;

/// Response structure from Vault's secret read operations.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
pub struct VaultReadOutput {
    /// Contains actual secret credentials.
    pub data: VaultCreds,
    /// Time-to-live duration in seconds for secret.
    lease_duration: i32,
    /// Lease identifier for secret life cycle management.
    lease_id: String,
    /// Indicates if lease can be renewed.
    renewable: bool,
    /// Unique request identifier for tracing.
    request_id: String,
    /// Non-critical operational warnings.
    warnings: Vec<String>,
}

/// Database credentials stored in Vault.
#[derive(Debug, Deserialize)]
pub struct VaultCreds {
    /// Database password.
    pub password: String,
    /// Database username.
    pub username: String,
}

/// Checks and renews Vault authentication using OIDC/Okta if token is invalid.
///
/// # Errors
/// - Executing `vault` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - OIDC/Okta login fails.
/// - Token lookup fails for a reason other than permission denied.
pub fn log_into_vault_if_required() -> rootcause::Result<()> {
    let token_lookup = Command::new("vault").args(["token", "lookup"]).output()?;
    if token_lookup.status.success() {
        return Ok(());
    }
    let stderr = std::str::from_utf8(&token_lookup.stderr)
        .context("error invalid utf-8 stderr")
        .attach_with(|| format!("cmd=\"vault token lookup\" stderr={:?}", token_lookup.stderr))?
        .trim();
    if !stderr.contains("permission denied") {
        Err(report!("error checking vault token")).attach_with(|| format!("stderr={stderr:?}"))?;
    }

    let login = Command::new("vault")
        .args(["login", "-method=oidc", "-path=okta", "--no-print"])
        .output()?;
    if !login.status.success() {
        let stderr = std::str::from_utf8(&login.stderr)
            .context("error invalid utf-8 stderr")
            .attach_with(|| format!("cmd=\"vault login\" stderr={:?}", login.stderr))?
            .trim();
        Err(report!("error logging into vault")).attach_with(|| format!("stderr={stderr:?}"))?;
    }

    Ok(())
}
