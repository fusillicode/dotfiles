use std::process::Command;

use color_eyre::eyre::bail;
use serde::Deserialize;

/// Response structure from Vault's secret read operations.
#[derive(Deserialize, Debug)]
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
#[derive(Deserialize, Debug)]
pub struct VaultCreds {
    /// Database password.
    pub password: String,
    /// Database username.
    ///
    ///
    /// Returns an error if:
    /// - Executing `vault` fails or returns non-zero exit status.
    /// - UTF-8 conversion fails.
    pub username: String,
}

/// Checks and renews Vault authentication using OIDC/Okta if token is invalid.
///
/// # Workflow
/// 1. Checks current Vault token validity via `vault token lookup`.
/// 2. If valid, returns immediately.
/// 3. If token is invalid due to permission error, initiates OIDC login via Okta.
/// 4. Fails on unexpected lookup errors or login failures.
///
/// # Errors
///
/// Returns an error if:
/// - Executing `vault` fails or returns a non-zero exit status.
/// - UTF-8 conversion fails.
/// - OIDC/Okta login fails.
/// - Token lookup fails for a reason other than permission denied.
pub fn log_into_vault_if_required() -> color_eyre::Result<()> {
    let token_lookup = Command::new("vault").args(["token", "lookup"]).output()?;
    if token_lookup.status.success() {
        return Ok(());
    }
    let stderr = std::str::from_utf8(&token_lookup.stderr)?.trim();
    if !stderr.contains("permission denied") {
        bail!("unexpected error checking Vault token - error {stderr}")
    }

    let login = Command::new("vault")
        .args(["login", "-method=oidc", "-path=okta", "--no-print"])
        .output()?;
    if !login.status.success() {
        bail!(
            "error authenticating to Vault - error {}",
            std::str::from_utf8(&login.stderr)?.trim()
        )
    }

    Ok(())
}
