use crate::utils::system::silent_cmd;

pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    silent_cmd("npm")
        .args(
            [
                &["install", "--silent", "--prefix", &dev_tools_repo_dir][..],
                packages,
            ]
            .concat(),
        )
        .status()?
        .exit_ok()?;

    Ok(silent_cmd("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/node_modules/.bin/{bin} {bin_dir}"),
        ])
        .status()?
        .exit_ok()?)
}
