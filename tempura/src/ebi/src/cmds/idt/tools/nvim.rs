use crate::cmds::idt::tools::Installer;
use utils::system::silent_cmd;

pub struct NvimInstaller {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for NvimInstaller {
    fn bin(&self) -> &'static str {
        "nvim"
    }

    fn install(&self) -> anyhow::Result<()> {
        // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
        // Moreover...it's pretty badass 😎
        let nvim_source_dir = format!("{}/{}/source", self.dev_tools_dir, self.bin());
        let nvim_release_dir = format!("{}/{}/release", self.dev_tools_dir, self.bin());

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
                self.bin(),
                self.bin_dir
            ),
        ])
        .status()?
        .exit_ok()?)
    }
}
