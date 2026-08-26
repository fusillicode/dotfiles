use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::deflate::ChecksumSource;
use crate::downloaders::http::deflate::HttpDeflateOption;
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
            Arch::Arm => "arm64",
            Arch::X86 => "x86_64",
        };
        (arch, os)
    }
}

impl Installer for Hadolint<'_> {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let filename = format!("{0}-{os}-{arch}", self.bin_name());
        let checksums_url = format!(
            "https://github.com/{0}/{0}/releases/latest/download/{filename}.sha256",
            self.bin_name()
        );

        let target = crate::downloaders::http::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{filename}",
                self.bin_name()
            ),
            &HttpDeflateOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
            Some(&ChecksumSource {
                checksums_url: &checksums_url,
                filename: &filename,
            }),
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}
