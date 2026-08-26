use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::deflate::HttpDeflateOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct TyposLsp<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for TyposLsp<'_> {
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

impl Installer for TyposLsp<'_> {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        let repo = "tekumara/typos-vscode";
        let latest_release = crate::downloaders::http::github::get_latest_release_tag(repo)?;

        let target = crate::downloaders::http::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-{arch}-{os}.tar.gz",
                self.bin_name()
            ),
            &HttpDeflateOption::ExtractTarGz {
                dest_dir: self.bin_dir,
                dest_name: Some(self.bin_name()),
            },
            None,
        )?;

        ytil_sys::file::chmod_x(target)?;

        Ok(())
    }
}
