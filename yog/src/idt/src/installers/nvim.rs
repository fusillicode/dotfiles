use crate::Installer;
use utils::cmd::silent_cmd;

pub struct Nvim {
    pub dev_tools_dir: String,
    pub bins_dir: String,
}

impl Installer for Nvim {
    fn bin_name(&self) -> &'static str {
        "nvim"
    }

    fn install(&self) -> color_eyre::Result<()> {
        // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
        // Moreover...it's pretty badass 😎
        let nvim_source_dir = format!("{}/{}/source", self.dev_tools_dir, self.bin_name());
        let nvim_release_dir = format!("{}/{}/release", self.dev_tools_dir, self.bin_name());

        Ok(silent_cmd("sh")
        .args([
            "-c",
            &format!(
                r#"
                    ([ ! -d "{nvim_source_dir}" ] && \
                        git clone https://github.com/neovim/neovim {nvim_source_dir} || true) && \
                    cd {nvim_source_dir} && \
                    git checkout master && \
                    git pull origin master && \
                    make distclean && \
                    make CMAKE_BUILD_TYPE=Release CMAKE_EXTRA_FLAGS="-DCMAKE_INSTALL_PREFIX={nvim_release_dir}" && \
                    make install && \
                    ln -sf {nvim_release_dir}/bin/{} {}
                "#,
                self.bin_name(),
                self.bins_dir
            ),
        ])
        .status()?
        .exit_ok()?)
    }
}
