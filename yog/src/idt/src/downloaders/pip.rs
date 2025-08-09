pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dest_dir: &str,
    bin_name: &str,
) -> color_eyre::Result<String> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    utils::cmd::silent_cmd("python3")
        .args(["-m", "venv", &format!("{dev_tools_repo_dir}/.venv")])
        .status()?
        .exit_ok()?;

    let bin_src = format!("{dev_tools_repo_dir}/.venv/bin/{bin_name}");

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                r#"
                    source {dev_tools_repo_dir}/.venv/bin/activate && \
                    pip install pip {packages} --upgrade && \
                    ln -sf {bin_src} {bin_dest_dir}
                "#,
                packages = packages.join(" "),
            ),
        ])
        .status()?
        .exit_ok()?;

    Ok(bin_src)
}
