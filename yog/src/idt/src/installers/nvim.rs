use crate::Installer;
use utils::cmd::silent_cmd;
use utils::system::symlink::Symlink;

pub struct Nvim {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Nvim {
    fn bin_name(&self) -> &'static str {
        "nvim"
    }

    fn download(&self) -> color_eyre::Result<Box<dyn Symlink>> {
        // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
        // Moreover...it's pretty badass 😎
        let nvim_source_dir = format!("{}/{}/source", self.dev_tools_dir, self.bin_name());
        let nvim_release_dir = format!("{}/{}/release", self.dev_tools_dir, self.bin_name());

        silent_cmd("sh")
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
                    make install
                "#,
            ),
        ])
        .status()?
        .exit_ok()?;

        let link = format!("{}/{}", self.bin_dir, self.bin_name());
        let symlink = utils::system::symlink::build(
            &format!("{nvim_release_dir}/bin/{}", self.bin_name()),
            Some(&link),
        )?;

        Ok(symlink)
    }
}
