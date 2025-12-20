use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::curl::CurlDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Shellcheck<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Shellcheck<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match arch {
            Arch::Arm => "aarch64",
            Arch::X86 => "x86_64",
        };
        (arch, os)
    }
}

impl Installer for Shellcheck<'_> {
    fn bin_name(&self) -> &'static str {
        "shellcheck"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let (arch, os) = self.target_arch_and_os();

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
