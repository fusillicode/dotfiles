use std::path::Path;

use rootcause::prelude::ResultExt as _;
use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::ChecksumSource;
use crate::downloaders::http::HttpDownloaderOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct TerraformLs<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for TerraformLs<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match arch {
            Arch::Arm => "arm64",
            Arch::X86 => "amd64",
        };
        (arch, os)
    }
}

impl Installer for TerraformLs<'_> {
    fn bin_name(&self) -> &'static str {
        "terraform-ls"
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let repo = format!("hashicorp/{}", self.bin_name());
        let latest_release_tag = ytil_gh::get_latest_release(&repo)?;
        let latest_release = latest_release_tag
            .get(1..)
            .ok_or_else(|| rootcause::report!("error trimming 'v' prefix from release tag"))
            .attach_with(|| format!("tag={latest_release_tag:?}"))?;

        let filename = format!("{0}_{latest_release}_{os}_{arch}.zip", self.bin_name());
        let checksums_url = format!(
            "https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_SHA256SUMS",
            self.bin_name()
        );

        let target = crate::downloaders::http::run(
            &format!(
                "https://releases.hashicorp.com/{0}/{latest_release}/{filename}",
                self.bin_name()
            ),
            &HttpDownloaderOption::ExtractTarGz {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
            Some(&ChecksumSource {
                checksums_url: &checksums_url,
                filename: &filename,
            }),
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
