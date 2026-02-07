use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::ChecksumSource;
use crate::downloaders::http::HttpDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Deno<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Deno<'_> {
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

impl Installer for Deno<'_> {
    fn bin_name(&self) -> &'static str {
        "deno"
    }

    fn install(&self) -> rootcause::Result<()> {
        let repo = format!("{0}land/{0}", self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;

        let (arch, os) = self.target_arch_and_os();

        let filename = format!("{}-{arch}-{os}.zip", self.bin_name());
        let checksums_url =
            format!("https://github.com/{repo}/releases/download/{latest_release}/{filename}.sha256sum");

        let target = crate::downloaders::http::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{filename}"),
            &HttpDownloaderOption::ExtractTarGz {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
            Some(&ChecksumSource {
                checksums_url: &checksums_url,
                filename: &filename,
            }),
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }
}
