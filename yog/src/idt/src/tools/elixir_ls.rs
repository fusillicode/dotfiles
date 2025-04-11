use std::process::Command;


use crate::installers::curl_install::OutputOption;
use crate::Installer;

pub struct ElixirLs {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for ElixirLs {
    fn bin_name(&self) -> &'static str {
        "elixir-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("elixir-lsp/{}", self.bin_name());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::installers::curl_install::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip", self.bin_name()),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C", &dev_tools_repo_dir])),
        )?;
        utils::system::chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
        utils::system::silent_cmd("ln")
            .args([
                "-sf",
                &format!("{dev_tools_repo_dir}/language_server.sh"),
                &format!("{}/{}", self.bin_dir, self.bin_name()),
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
