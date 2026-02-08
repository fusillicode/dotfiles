use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::HttpDeflateOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct HelmLs<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for HelmLs<'_> {
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

impl Installer for HelmLs<'_> {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();
        let repo = "mrjosh/helm-ls";
        let latest_release = ytil_gh::get_latest_release(repo)?;

        let target = crate::downloaders::http::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}_{os}_{arch}",
                self.bin_name()
            ),
            &HttpDeflateOption::WriteTo {
                dest_path: &self.bin_dir.join(self.bin_name()),
            },
            None,
        )?;

        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["version"])
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
