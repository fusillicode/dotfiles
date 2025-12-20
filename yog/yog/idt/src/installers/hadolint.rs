use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::curl::CurlDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Hadolint<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Hadolint<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            Os::MacOs => "macos",
            Os::Linux => "linux",
        };
        let arch = match arch {
            Arch::Arm => "arm",
            Arch::X86 => "x86_64",
        };
        (arch, os)
    }
}

impl Installer for Hadolint<'_> {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let (arch, os) = self.target_arch_and_os();

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
