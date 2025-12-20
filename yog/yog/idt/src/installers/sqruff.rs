use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::curl::CurlDownloaderOption;
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

    fn install(&self) -> color_eyre::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-{os}-{arch}.tar.gz",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoTar {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
