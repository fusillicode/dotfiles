use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::curl::CurlDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Marksman<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Marksman<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let arch_suffix = match (os, arch) {
            (Os::MacOs, Arch::Arm | Arch::X86) => "",
            (Os::Linux, Arch::Arm) => "-arm64",
            (Os::Linux, Arch::X86) => "-x64",
        };
        let os = match os {
            Os::MacOs => "macos",
            Os::Linux => "linux",
        };
        (arch_suffix, os)
    }
}

impl Installer for Marksman<'_> {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-{os}{arch}",
                self.bin_name()
            ),
            &CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }
}
