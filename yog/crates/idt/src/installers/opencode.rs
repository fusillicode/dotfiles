use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::deflate::HttpDeflateOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct Opencode<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for Opencode<'_> {
    fn target_arch_and_os(&self) -> (&str, &str) {
        let SysInfo { os, arch } = self.sys_info;
        let os = match os {
            Os::MacOs => "darwin",
            Os::Linux => "linux",
        };
        let arch = match arch {
            Arch::Arm => "arm64",
            Arch::X86 => "x64",
        };
        (arch, os)
    }
}

impl Installer for Opencode<'_> {
    fn bin_name(&self) -> &'static str {
        "opencode"
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }

    fn install(&self) -> rootcause::Result<()> {
        let repo = format!("anomalyco/{}", self.bin_name());
        let latest_release = crate::downloaders::http::github::get_latest_release_tag(&repo)?;

        let (arch, os) = self.target_arch_and_os();

        let (filename, deflate_option) = match self.sys_info.os {
            Os::MacOs => (
                format!("{}-{os}-{arch}.zip", self.bin_name()),
                HttpDeflateOption::ExtractZip {
                    dest_dir: self.bin_dir,
                    dest_name: Some(self.bin_name()),
                },
            ),
            Os::Linux => (
                format!("{}-{os}-{arch}.tar.gz", self.bin_name()),
                HttpDeflateOption::ExtractTarGz {
                    dest_dir: self.bin_dir,
                    dest_name: Some(self.bin_name()),
                },
            ),
        };

        let target = crate::downloaders::http::run(
            &format!("https://github.com/{repo}/releases/download/{latest_release}/{filename}"),
            &deflate_option,
            None,
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
