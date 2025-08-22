use std::path::Path;
use std::path::PathBuf;

#[allow(dead_code)]
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> color_eyre::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    utils::cmd::silent_cmd("composer")
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

    Ok(dev_tools_repo_dir.join("vendor/bin"))
}
