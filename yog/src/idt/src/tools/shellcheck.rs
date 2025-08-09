use std::process::Command;

use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct Shellcheck {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Shellcheck {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz",
                self.bin_name()
            ),
            OutputOption::PipeInto(Command::new("tar").args(["-xz", "-C"]), "/tmp".into()),
        )?;

        utils::cmd::silent_cmd("mv")
            .args([
                &format!("/tmp/{0}-{latest_release}/{0}", self.bin_name()),
                &self.bin_dest_dir,
            ])
            .status()?
            .exit_ok()?;

        Ok(None)
    }
}
