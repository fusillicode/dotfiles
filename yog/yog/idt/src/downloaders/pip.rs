use std::path::Path;
use std::path::PathBuf;

/// Downloads and installs Python packages using pip in a virtual environment.
///
/// # Errors
/// In case:
/// - Executing the `python3 -m venv` command fails or returns a non-zero exit status.
/// - Executing the shell pipeline to activate the venv and install packages fails.
/// - A filesystem operation (create/read/write/remove) fails.
pub fn run(dev_tools_dir: &Path, tool: &str, packages: &[&str]) -> color_eyre::Result<PathBuf> {
    let dev_tools_repo_dir = dev_tools_dir.join(tool);

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    ytil_cmd::silent_cmd("python3")
        .args(["-m", "venv", &dev_tools_repo_dir.join(".venv").to_string_lossy()])
        .status()?
        .exit_ok()?;

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
        .status()?
        .exit_ok()?;

    Ok(dev_tools_repo_dir.join(".venv").join("bin"))
}
