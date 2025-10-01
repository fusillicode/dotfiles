use std::path::Path;
use std::path::PathBuf;

#[expect(dead_code, reason = "Kept for memories")]
/// Downloads and installs PHP packages using Composer.
///
/// # Errors
/// Returns an error if:
/// - Executing the `composer` command fails or returns a non-zero exit status.
/// - A filesystem operation (create/read/write/remove) fails.
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> color_eyre::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    ytil_cmd::silent_cmd("composer")
        .args(
            [
                &[
                    "require",
                    "--dev",
                    "--working-dir",
                    &dev_tools_repo_dir.to_string_lossy(),
                ][..],
                packages,
            ]
            .concat(),
        )
        .status()?
        .exit_ok()?;

    Ok(dev_tools_repo_dir.join("vendor").join("bin"))
}
