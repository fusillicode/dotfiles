use std::process::Command;

use crate::installers::curl_install::OutputOption;
use crate::Installer;

pub struct LuaLanguageServer {
    pub dev_tools_dir: String,
}

impl Installer for LuaLanguageServer {
    fn bin_name(&self) -> &'static str {
        "lua-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
        // the `bin` there.
        let repo = format!("LuaLS/{}", self.bin_name());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::installers::curl_install::run(
           &format!("https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-darwin-arm64.tar.gz", self.bin_name()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
        )
    }
}
