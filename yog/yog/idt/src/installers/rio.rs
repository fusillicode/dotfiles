use std::path::Path;

use ytil_cmd::silent_cmd;

use crate::installers::Installer;

pub struct Rio<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Rio<'_> {
    fn bin_name(&self) -> &'static str {
        "rio"
    }

    fn install(&self) -> rootcause::Result<()> {
        let source_dir = self.dev_tools_dir.join(self.bin_name()).join("source");

        silent_cmd("sh")
            .args([
                "-c",
                &format!(
                    r#"
                        ([ ! -d "{0}" ] && \
                            git clone --depth=1 https://github.com/raphamorim/rio.git {0} || true) && \
                        cd {0} && \
                        git fetch origin main --depth=1 && \
                        git checkout origin/main && \
                        rustup target add aarch64-apple-darwin && \
                        make release-macos
                    "#,
                    source_dir.display(),
                ),
            ])
            .status()?
            .exit_ok()?;

        let app = source_dir.join("release").join("Rio.app");
        let binary = app.join("Contents").join("MacOS").join("rio");
        ytil_sys::file::ln_sf(&binary, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(&binary)?;

        // Symlink into /Applications so Spotlight can find it.
        let applications_link = Path::new("/Applications/Rio.app").to_path_buf();
        ytil_sys::file::ln_sf(&app, &applications_link)?;

        Ok(())
    }

    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
