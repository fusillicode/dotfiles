use std::path::Path;

use rootcause::prelude::ResultExt;

use crate::Installer;

pub struct Nvim<'a> {
    pub dev_tools_dir: &'a Path,
    pub bin_dir: &'a Path,
}

impl Installer for Nvim<'_> {
    fn bin_name(&self) -> &'static str {
        "nvim"
    }

    fn install(&self) -> rootcause::Result<()> {
        // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
        // Moreover...it's pretty badass 😎
        let nvim_source_dir = self.dev_tools_dir.join(self.bin_name()).join("source");
        let nvim_release_dir = self.dev_tools_dir.join(self.bin_name()).join("release");

        ytil_cmd::silent_cmd("sh")
            .args([
                "-c",
                &format!(
                    r#"
                        ([ ! -d "{0}" ] && \
                            git clone https://github.com/neovim/neovim {0} || true) && \
                        cd {0} && \
                        git checkout master && \
                        git pull origin master && \
                        make distclean && \
                        make CMAKE_BUILD_TYPE=Release CMAKE_EXTRA_FLAGS="-DCMAKE_INSTALL_PREFIX={1}" && \
                        make install
                    "#,
                    nvim_source_dir.display(),
                    nvim_release_dir.display(),
                ),
            ])
            .status()
            .context("failed to spawn build command")?
            .exit_ok()
            .context("build failed")
            .attach_with(|| format!("tool={}", self.bin_name()))
            .attach_with(|| format!("source_dir={}", nvim_source_dir.display()))?;

        let target = nvim_release_dir.join("bin").join(self.bin_name());
        ytil_sys::file::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        ytil_sys::file::chmod_x(&target)?;

        Ok(())
    }

    fn health_check_args(&self) -> Option<&[&str]> {
        Some(&["-V1", "-v"])
    }
}
