use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;

pub struct LuaLsInstaller {
    pub dev_tools_dir: String,
}

impl Installer for LuaLsInstaller {
    fn tool(&self) -> &'static str {
        "lua_ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
        // the `bin` there.
        let tool = "lua-language-server";
        let repo = format!("LuaLS/{tool}");
        let dev_tools_repo_dir = format!("{}/{tool}", self.dev_tools_dir);
        let latest_release = crate::utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;
        crate::cmds::install_dev_tools::curl_install::run(
        &format!("https://github.com/{repo}/releases/download/{latest_release}/{tool}-{latest_release}-darwin-arm64.tar.gz"),
        OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
    )
    }
}
