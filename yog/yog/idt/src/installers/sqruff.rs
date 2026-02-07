use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::HttpDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Sqruff<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Sqruff<'_> {
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

impl Installer for Sqruff<'_> {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }

    fn install(&self) -> color_eyre::Result<()> {
        let (arch, os) = self.target_arch_and_os();
        let repo = format!("quarylabs/{}", self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;

        let target = crate::downloaders::http::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{0}-{os}-{arch}.tar.gz",
                self.bin_name()
            ),
            &HttpDownloaderOption::ExtractTarGz {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
            None,
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
