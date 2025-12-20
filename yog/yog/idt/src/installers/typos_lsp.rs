use std::path::Path;

use ytil_sys::SysInfo;

use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct TyposLsp<'a> {
    pub bin_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl Installer for TyposLsp<'_> {
    fn bin_name(&self) -> &'static str {
        "typos-lsp"
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

        let repo = "tekumara/typos-vscode";
        let latest_release = ytil_gh::get_latest_release(repo)?;

        let target = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-{arch}-{os}.tar.gz",
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
