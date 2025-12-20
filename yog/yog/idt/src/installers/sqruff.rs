use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct Sqruff<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for Sqruff<'_> {
    fn bin_name(&self) -> &'static str {
        "sqruff"
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
