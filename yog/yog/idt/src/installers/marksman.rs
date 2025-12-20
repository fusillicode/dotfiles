use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Marksman<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for Marksman<'_> {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let SysInfo { os, arch } = self.sys_info;
        let arch = match (os, arch) {
            (ytil_sys::Os::MacOs, ytil_sys::Arch::Arm | ytil_sys::Arch::X86) => "",
            (ytil_sys::Os::Linux, ytil_sys::Arch::Arm) => "-arm64",
            (ytil_sys::Os::Linux, ytil_sys::Arch::X86) => "-x64",
        };
        let os = match os {
            ytil_sys::Os::MacOs => "macos",
            ytil_sys::Os::Linux => "linux",
        };

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
