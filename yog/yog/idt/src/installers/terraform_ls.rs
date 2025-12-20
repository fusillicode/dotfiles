use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TerraformLs<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for TerraformLs<'_> {
    fn bin_name(&self) -> &'static str {
        "terraform-ls"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            ytil_sys::Os::MacOs => "darwin",
            ytil_sys::Os::Linux => "linux",
        };
        let arch = match arch {
            ytil_sys::Arch::Arm => "arm64",
            ytil_sys::Arch::X86 => "amd64",
        };

        let repo = format!("hashicorp/{}", self.bin_name());
        let latest_release = &ytil_gh::get_latest_release(&repo)?[1..];

        let target = crate::downloaders::curl::run(
            &format!(
                "https://releases.hashicorp.com/{0}/{latest_release}/{0}_{latest_release}_{os}_{arch}.zip",
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
