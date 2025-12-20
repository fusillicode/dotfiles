use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct RustAnalyzer<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for RustAnalyzer<'_> {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            ytil_sys::Os::MacOs => "apple-darwin",
            ytil_sys::Os::Linux => "unknown-linux",
        };
        let arch = match arch {
            ytil_sys::Arch::Arm => "aarch64",
            ytil_sys::Arch::X86 => "x86_64",
        };

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-{arch}-{os}.gz",
                self.bin_name()
            ),
            &CurlDownloaderOption::PipeIntoZcat {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
