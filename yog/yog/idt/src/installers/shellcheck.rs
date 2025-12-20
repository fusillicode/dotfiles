use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Shellcheck<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for Shellcheck<'_> {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            ytil_sys::Os::MacOs => "darwin",
            ytil_sys::Os::Linux => "linux",
        };
        let arch = match arch {
            ytil_sys::Arch::Arm => "aarch64",
            ytil_sys::Arch::X86 => "x86_64",
        };

        let repo = format!("koalaman/{}", self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;
        let dest_dir = Path::new("/tmp");

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.{os}.{arch}.tar.xz",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir,
                dest_name: None,
            },
        )?;

        let target = self.bin_dir.join(self.bin_name());
        std::fs::rename(
            dest_dir
                .join(format!("{0}-{latest_release}", self.bin_name()))
                .join(self.bin_name()),
            &target,
        )?;
        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
