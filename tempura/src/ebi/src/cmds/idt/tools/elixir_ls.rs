use std::process::Command;

use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;
use crate::utils::system::silent_cmd;

pub struct ElixirLsInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElixirLsInstaller {
    fn bin(&self) -> &'static str {
        "elixir-ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        let repo = format!("elixir-lsp/{}", self.bin());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin());
        let latest_release = crate::utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::cmds::idt::curl_install::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip", self.bin()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
        )?;
        crate::utils::system::chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
        silent_cmd("ln")
            .args([
                "-sf",
                &format!("{dev_tools_repo_dir}/language_server.sh"),
                &format!("{}/{}", self.bin_dir, self.bin()),
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
