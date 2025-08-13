use utils::system::symlink::Symlink;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Shellcheck {
    pub bin_dir: String,
}

impl Installer for Shellcheck {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.darwin.x86_64.tar.xz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: "/tmp",
                dest_name: None,
            },
        )?;

        let target = format!("{}/{}", self.bin_dir, self.bin_name());

        std::fs::rename(
            format!("/tmp/{0}-{latest_release}/{0}", self.bin_name()),
            &target,
        )?;

        let symlink = utils::system::symlink::build(&target, None)?;

        Ok(symlink)
    }
}
