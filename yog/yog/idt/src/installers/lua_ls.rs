use std::path::Path;

use ytil_sys::Arch;
use ytil_sys::Os;
use ytil_sys::SysInfo;

use crate::downloaders::http::HttpDeflateOption;
use crate::installers::Installer;
use crate::installers::SystemDependent;

pub struct LuaLanguageServer<'a> {
    pub dev_tools_dir: &'a Path,
    pub sys_info: &'a SysInfo,
}

impl SystemDependent for LuaLanguageServer<'_> {
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

impl Installer for LuaLanguageServer<'_> {
    fn bin_name(&self) -> &'static str {
        "lua-language-server"
    }

    fn install(&self) -> rootcause::Result<()> {
        let (arch, os) = self.target_arch_and_os();

        // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to
        // point to the `bin` there.
        let repo = format!("LuaLS/{}", self.bin_name());
        let dev_tools_repo_dir = self.dev_tools_dir.join(self.bin_name());
        let latest_release = ytil_gh::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        let target_dir = crate::downloaders::http::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-{os}-{arch}.tar.gz",
                self.bin_name()
            ),
            &HttpDeflateOption::ExtractTarGz {
                dest_dir: &dev_tools_repo_dir,
                dest_name: None,
            },
            None,
        )?;

        ytil_sys::file::chmod_x(target_dir.join("bin").join(self.bin_name()))?;

        Ok(())
    }

    // NOTE: skip because it's a shitshow...
    fn check_args(&self) -> Option<&[&str]> {
        None
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
