use crate::Installer;
use crate::downloaders::curl::InstallOption;

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

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip",
                self.bin_name()
            ),
            InstallOption::PipeToTar {
                dest_dir: &dev_tools_repo_dir,
                dest_name: self.bin_name(),
            },
        )?;
        utils::system::chmod_x(&format!("{dev_tools_repo_dir}/*"))?;
        utils::cmd::silent_cmd("ln")
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
