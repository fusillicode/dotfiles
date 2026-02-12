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

    /// Builds Rio from source, symlinks the binary into `bin_dir`, and
    /// copies `Rio.app` into `/Applications` (atomic swap).
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

        crate::installers::install_macos_app(&app, self.bin_dir, self.bin_name())?;

        Ok(())
    }

    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }

    fn should_verify_checksum(&self) -> bool {
        false
    }
}
