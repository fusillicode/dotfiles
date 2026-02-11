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

    /// Builds Rio from source and installs it.
    ///
    /// 1. Clones (or updates) the Rio repository into the dev-tools source directory and runs `make release-macos` to
    ///    produce the `Rio.app` bundle.
    /// 2. Symlinks the binary into `bin_dir` so `rio` is available on `$PATH`.
    /// 3. Copies the `.app` bundle into `/Applications` so Spotlight can index it.
    ///
    /// `/Applications/Rio.app` may already exist as one of three things:
    ///   - a symlink (unlikely, but possible leftover)
    ///   - a Finder alias from an older version of this installer (a regular file)
    ///   - a real `.app` directory from a previous copy
    ///
    /// The existing entry is moved aside (not deleted) before the new bundle is
    /// copied in, so that a Rio instance launched from `/Applications` keeps
    /// running while the swap happens. The old entry is cleaned up after the
    /// copy. `remove_dir_all` works on both directories and plain files (such
    /// as a Finder alias).
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

        let applications_app = Path::new("/Applications/Rio.app");
        let applications_app_old = Path::new("/Applications/Rio.app.old");
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
