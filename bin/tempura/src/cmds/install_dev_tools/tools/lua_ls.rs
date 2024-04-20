use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;

pub fn install(dev_tools_dir: &str) -> anyhow::Result<()> {
    // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
    // the `bin` there.
    let tool = "lua-language-server";
    let repo = format!("LuaLS/{tool}");
    let dev_tools_repo_dir = format!("{dev_tools_dir}/{tool}");
    let latest_release = crate::utils::github::get_latest_release(&repo)?;
    std::fs::create_dir_all(&dev_tools_repo_dir)?;
    crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}-darwin-arm64.tar.gz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )
}
