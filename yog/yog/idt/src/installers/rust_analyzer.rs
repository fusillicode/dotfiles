use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::HttpDeflateOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct RustAnalyzer<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for RustAnalyzer<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            Os::MacOs => "apple-darwin",
            Os::Linux => "unknown-linux",
        };
        let arch = match arch {
            Arch::Arm => "aarch64",
            Arch::X86 => "x86_64",
        };
        (arch, os)
    }
}

impl Installer for RustAnalyzer<'_> {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let target = crate::downloaders::http::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-{arch}-{os}.gz",
                self.bin_name()
            ),
            &HttpDeflateOption::DecompressGz {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
            None,
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
