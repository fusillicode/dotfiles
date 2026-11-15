//! Concrete tool installers and their shared installation helpers.

use std::path::Path;
use std::path::PathBuf;

use rootcause::bail;
use rootcause::prelude::ResultExt;

use crate::installer::InstallOutcome;

pub mod alacritty;
pub mod bash_language_server;
pub mod cargo;
pub mod commitlint;
pub mod deno;
pub mod docker_langserver;
pub mod eslint_d;
pub mod graphql_lsp;
pub mod hadolint;
pub mod helm_ls;
pub mod lua_ls;
pub mod marksman;
pub mod nvim;
pub mod opencode;
pub mod prettierd;
pub mod quicktype;
pub mod rio;
pub mod ruff_lsp;
pub mod shellcheck;
pub mod sql_language_server;
pub mod starship;
pub mod terraform_ls;
pub mod typescript_language_server;
pub mod vscode_langservers;
pub mod yaml_language_server;
pub mod zellij;

/// Common install pattern for npm-based tools: download via npm, symlink the binary, and make it executable.
///
/// # Errors
/// - If npm download fails.
/// - If symlink creation fails.
/// - If chmod fails.
pub fn install_npm_tool(
    dev_tools_dir: &Path,
    bin_dir: &Path,
    bin_name: &str,
    npm_name: &str,
    packages: &[&str],
) -> rootcause::Result<InstallOutcome> {
    let target_dir = crate::downloaders::npm::run(dev_tools_dir, npm_name, packages)?;
    let target = target_dir.join(bin_name);
    ytil_sys::file::ln_sf(&target, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(target)?;
    Ok(InstallOutcome::ChecksumNotNeeded)
}

/// Common install pattern for macOS `.app` bundles built from source.
///
/// 1. symlink the binary into `bin_dir`
/// 2. make it executable
/// 3. copy the `.app` bundle into `/Applications` with an atomic swap
///
/// # Errors
/// - If symlink creation fails.
/// - If chmod fails.
/// - If copy to `/Applications` fails.
pub fn install_macos_app(app: &Path, bin_dir: &Path, bin_name: &str) -> rootcause::Result<()> {
    let binary = app.join("Contents").join("MacOS").join(bin_name);

    ytil_sys::file::ln_sf(&binary, &bin_dir.join(bin_name))?;
    ytil_sys::file::chmod_x(&binary)?;

    let Some(app_filename) = app.file_name().and_then(|n| n.to_str()) else {
        bail!("app path has no valid UTF-8 file name: {}", app.display());
    };

    let applications_app = PathBuf::from(format!("/Applications/{app_filename}"));
    let applications_app_old = PathBuf::from(format!("/Applications/{app_filename}.old"));

    if applications_app_old.exists() {
        std::fs::remove_dir_all(&applications_app_old)
            .context("error removing old .app backup")
            .attach_with(|| format!("path={}", applications_app_old.display()))?;
    }

    if applications_app.is_symlink() {
        std::fs::remove_file(&applications_app)
            .context("error removing .app symlink")
            .attach_with(|| format!("path={}", applications_app.display()))?;
    } else if applications_app.exists() {
        std::fs::rename(&applications_app, &applications_app_old)
            .context("error renaming existing .app to .old")
            .attach_with(|| format!("from={}", applications_app.display()))
            .attach_with(|| format!("to={}", applications_app_old.display()))?;
    }

    ytil_cmd::silent_cmd("cp")
        .args(["-R", &app.display().to_string(), "/Applications/"])
        .status()
        .context("failed to spawn cp for .app bundle")?
        .exit_ok()
        .context("cp .app bundle to /Applications failed")
        .attach_with(|| format!("app={}", app.display()))?;

    if applications_app_old.exists() {
        std::fs::remove_dir_all(&applications_app_old)
            .context("error cleaning up old .app backup")
            .attach_with(|| format!("path={}", applications_app_old.display()))?;
    }

    Ok(())
}
