use std::process::Command;

use crate::cmds::install_dev_tools::curl_install::OutputOption;
use crate::cmds::install_dev_tools::tools::Installer;
use crate::utils::system::silent_cmd;

pub struct ElixirLsInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElixirLsInstaller {
    fn tool(&self) -> &'static str {
        "elixir_ls"
    }

    fn install(&self) -> anyhow::Result<()> {
        let tool = "elixir-ls";
        let repo = format!("elixir-lsp/{tool}");
        let dev_tools_repo_dir = format!("{}/{tool}", self.dev_tools_dir);
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
                &format!("{}/elixir-ls", self.bin_dir),
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
