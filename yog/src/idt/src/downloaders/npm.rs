pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dest_dir: &str,
    bin_name: &str,
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

    let bin_src = format!("{dev_tools_repo_dir}/node_modules/.bin/{bin_name}");

    utils::cmd::silent_cmd("sh")
        .args(["-c", &format!("ln -sf {bin_src} {bin_dest_dir}")])
        .status()?
        .exit_ok()?;

    Ok(bin_src)
}
