pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> color_eyre::Result<String> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    let mut cmd_args = vec!["install"];
    if cfg!(debug_assertions) {
        cmd_args.push("--silent");
    }
    cmd_args.extend_from_slice(&["--prefix", &dev_tools_repo_dir]);
    cmd_args.extend_from_slice(packages);

    utils::cmd::silent_cmd("npm")
        .args(cmd_args)
        .status()?
        .exit_ok()?;

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/node_modules/.bin/{bin} {bin_dir}"),
        ])
        .status()?
        .exit_ok()?;

    Ok(format!("{dev_tools_repo_dir}/node_modules/.bin/{bin}"))
}
