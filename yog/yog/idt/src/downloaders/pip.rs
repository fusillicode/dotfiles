use std::path::Path;
use std::path::PathBuf;

use rootcause::prelude::ResultExt;

/// Downloads and installs Python packages using pip in a virtual environment.
///
/// # Errors
/// - Executing the `python3 -m venv` command fails or returns a non-zero exit status.
/// - Executing the shell pipeline to activate the venv and install packages fails.
/// - A filesystem operation (create/read/write/remove) fails.
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> rootcause::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)
        .context("error creating pip tool directory")
        .attach_with(|| format!("path={}", dev_tools_repo_dir.display()))?;

    ytil_cmd::silent_cmd("python3")
        .args(["-m", "venv", &dev_tools_repo_dir.join(".venv").to_string_lossy()])
        .status()
        .context("failed to spawn python3 venv")?
        .exit_ok()
        .context("python3 venv creation failed")
        .attach_with(|| format!("tool={tool}"))
        .attach_with(|| format!("venv_dir={}", dev_tools_repo_dir.join(".venv").display()))?;

    ytil_cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                r"
                    source {}/.venv/bin/activate && \
                    pip install pip {packages} --upgrade
                ",
                dev_tools_repo_dir.display(),
                packages = packages.join(" "),
            ),
        ])
        .status()
        .context("failed to spawn pip install")?
        .exit_ok()
        .context("pip install failed")
        .attach_with(|| format!("tool={tool}"))
        .attach_with(|| format!("packages={packages:?}"))?;

    Ok(dev_tools_repo_dir.join(".venv").join("bin"))
}
