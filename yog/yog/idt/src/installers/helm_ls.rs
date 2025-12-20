use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct HelmLs<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for HelmLs<'_> {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
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

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_{os}_{arch}",
                self.bin_name()
            ),
            &CurlDownloaderOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["version"])
    }
}
