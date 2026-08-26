use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;

/// Downloads and installs Node.js packages using npm.
///
/// # Errors
/// - Executing the `npm` command fails or returns a non-zero exit status.
/// - A filesystem operation (create/read/write/remove) fails.
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> rootcause::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)
        .context("error creating npm tool directory")
        .attach_with(|| format!("path={}", dev_tools_repo_dir.display()))?;

    let mut cmd_args = vec!["install"];
    if cfg!(debug_assertions) {
        cmd_args.push("--silent");
    }
    let dev_tools_repo_dir_bind = dev_tools_repo_dir.to_string_lossy();
    cmd_args.extend_from_slice(&["--prefix", &dev_tools_repo_dir_bind]);
    cmd_args.extend_from_slice(packages);

    ytil_cmd::silent_cmd("npm")
        .args(cmd_args)
        .status()
        .context("failed to spawn npm")?
        .exit_ok()
        .context("npm install failed")
        .attach_with(|| format!("tool={tool}"))
        .attach_with(|| format!("packages={packages:?}"))?;

    Ok(dev_tools_repo_dir.join("node_modules").join(".bin"))
}
