pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    utils::system::silent_cmd("python3")
        .args(["-m", "venv", &format!("{dev_tools_repo_dir}/.venv")])
        .status()?
        .exit_ok()?;

    Ok(utils::system::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                r#"
                    source {dev_tools_repo_dir}/.venv/bin/activate && \
                    pip install pip {packages} --upgrade && \
                    ln -sf {dev_tools_repo_dir}/.venv/bin/{bin} {bin_dir}
                "#,
                packages = packages.join(" "),
            ),
        ])
        .status()?
        .exit_ok()?)
}
