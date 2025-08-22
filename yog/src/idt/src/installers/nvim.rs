use std::path::PathBuf;

use utils::cmd::silent_cmd;

use crate::Installer;

pub struct Nvim {
    pub dev_tools_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl Installer for Nvim {
    fn bin_name(&self) -> &'static str {
        "nvim"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
        // Moreover...it's pretty badass 😎
        let nvim_source_dir = self.dev_tools_dir.join(self.bin_name()).join("source");
        let nvim_release_dir = self.dev_tools_dir.join(self.bin_name()).join("release");

        silent_cmd("sh")
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
            .status()?
            .exit_ok()?;

        let target = nvim_release_dir.join("bin").join(self.bin_name());
        utils::system::ln_sf(&target, &self.bin_dir.join(self.bin_name()))?;
        utils::system::chmod_x(&target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["-V1", "-v"])
    }
}
