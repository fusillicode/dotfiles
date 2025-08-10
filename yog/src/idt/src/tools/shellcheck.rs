use crate::Installer;
use crate::downloaders::curl::InstallOption;

pub struct Shellcheck {
    pub bin_dir: String,
}

impl Installer for Shellcheck {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz",
                self.bin_name()
            ),
            InstallOption::PipeToTar {
                dest_dir: "/tmp",
                dest_name: self.bin_name(),
            },
        )?;

        utils::cmd::silent_cmd("mv")
            .args([
                &format!("/tmp/{0}-{latest_release}/{0}", self.bin_name()),
                &self.bin_dir,
            ])
            .status()?
            .exit_ok()?;

        Ok(())
    }
}
