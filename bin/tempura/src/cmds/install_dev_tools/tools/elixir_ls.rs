use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::utils::system::silent_cmd;

pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    let tool = "elixir-ls";
    let repo = format!("elixir-lsp/{tool}");
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    let latest_release = crate::utils::github::get_latest_release(&repo)?;
    std::fs::create_dir_all(&dev_tools_repo_dir)?;
    crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}.zip"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )?;
    crate::utils::system::chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
    silent_cmd("ln")
        .args([
            "-sf",
            &format!("{dev_tools_repo_dir}/language_server.sh"),
            &format!("{bin_dir}/elixir-ls"),
        ])
        .status()?
        .exit_ok()?;

    Ok(())
}
