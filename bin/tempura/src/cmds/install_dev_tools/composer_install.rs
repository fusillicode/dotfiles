use std::process::Command;

pub fn composer_install(
    dev_tools_dir: &str,
    tool: &str,
    packages: &[&str],
    bin_dir: &str,
    bin: &str,
) -> anyhow::Result<()> {
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");

    std::fs::create_dir_all(&dev_tools_repo_dir)?;

    Command::new("composer")
        .args(
            [
                &["require", "--dev", "--working-dir", &dev_tools_repo_dir][..],
                packages,
            ]
            .concat(),
        )
        .status()?
        .exit_ok()?;

    Ok(Command::new("sh")
        .args([
            "-c",
            &format!("ln -sf {dev_tools_repo_dir}/vendor/bin/{bin} {bin_dir}"),
        ])
        .status()?
        .exit_ok()?)
}
