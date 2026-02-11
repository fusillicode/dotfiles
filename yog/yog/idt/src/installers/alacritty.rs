use std::path::Path;

use ytil_cmd::silent_cmd;

use crate::installers::Installer;

pub struct Alacritty<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Alacritty<'_> {
    fn bin_name(&self) -> &'static str {
        "alacritty"
    }

    /// Builds Alacritty from source, symlinks the binary into `bin_dir`, and
    /// copies `Alacritty.app` into `/Applications` (atomic swap).
    fn install(&self) -> rootcause::Result<()> {
        let source_dir = self.dev_tools_dir.join(self.bin_name()).join("source");

        silent_cmd("sh")
            .args([
                "-c",
                &format!(
                    r#"
                        ([ ! -d "{0}" ] && \
                            git clone --depth=1 https://github.com/alacritty/alacritty.git {0} || true) && \
                        cd {0} && \
                        git fetch origin master --depth=1 && \
                        git checkout origin/master && \
                        rustup toolchain install stable --profile default && \
                        rustup override set stable && \
                        make app
                    "#,
                    source_dir.display(),
                ),
            ])
            .status()?
            .exit_ok()?;

        let app = source_dir
            .join("target")
            .join("release")
            .join("osx")
            .join("Alacritty.app");
        let binary = app.join("Contents").join("MacOS").join("alacritty");
        ytil_sys::file::ln_sf(&binary, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(&binary)?;

        let applications_app = Path::new("/Applications/Alacritty.app");
        let applications_app_old = Path::new("/Applications/Alacritty.app.old");
        if applications_app_old.exists() {
            std::fs::remove_dir_all(applications_app_old)?;
        }
        if applications_app.is_symlink() {
            std::fs::remove_file(applications_app)?;
        } else if applications_app.exists() {
            std::fs::rename(applications_app, applications_app_old)?;
        }
        silent_cmd("cp")
            .args(["-R", &app.display().to_string(), "/Applications/"])
            .status()?
            .exit_ok()?;
        if applications_app_old.exists() {
            std::fs::remove_dir_all(applications_app_old)?;
        }

        Ok(())
    }

    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
