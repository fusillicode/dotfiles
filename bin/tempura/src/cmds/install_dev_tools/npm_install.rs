use std::process::Command;

pub fn run(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    Command::new("npm")
        .args(
            [
                &["install", "--silent", "--prefix", &dev_tools_repo_dir][..],
                packages,
            ]
            .concat(),
        )
        .status()?
        .exit_ok()?;

    Ok(Command::new("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/node_modules/.bin/{bin} {bin_dir}"),
        ])
        .status()?
        .exit_ok()?)
}
