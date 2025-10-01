use std::path::Path;
use std::path::PathBuf;

/// Downloads and installs Node.js packages using npm.
///
/// # Errors
/// In case:
/// - Executing the `npm` command fails or returns a non-zero exit status.
/// - A filesystem operation (create/read/write/remove) fails.
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> color_eyre::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    let mut cmd_args = vec!["install"];
    if cfg!(debug_assertions) {
        cmd_args.push("--silent");
    }
    let dev_tools_repo_dir_bind = dev_tools_repo_dir.to_string_lossy();
    cmd_args.extend_from_slice(&["--prefix", &dev_tools_repo_dir_bind]);
    cmd_args.extend_from_slice(packages);

    ytil_cmd::silent_cmd("npm").args(cmd_args).status()?.exit_ok()?;

    Ok(dev_tools_repo_dir.join("node_modules").join(".bin"))
}
