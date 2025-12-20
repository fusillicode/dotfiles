use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Hadolint<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for Hadolint<'_> {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            ytil_sys::Os::MacOs => "macos",
            ytil_sys::Os::Linux => "linux",
        };
        let arch = match arch {
            ytil_sys::Arch::Arm => "arm",
            ytil_sys::Arch::X86 => "x86_64",
        };

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-{os}-{arch}",
                self.bin_name()
            ),
            &CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    // NOTE: skip because hadolint started to segfault...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }
}
