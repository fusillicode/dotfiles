use std::process::Command;

pub fn install(dev_tools_dir: &str, bin_dir: &str) -> anyhow::Result<()> {
    // Compiling from sources because I can checkout specific refs in case of broken nightly builds.
    // Moreover...it's pretty badass 😎
    let nvim_source_dir = format!("{dev_tools_dir}/nvim/source");
    let nvim_release_dir = format!("{dev_tools_dir}/nvim/release");

    Ok(Command::new("sh")
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
                    ln -sf {nvim_release_dir}/bin/nvim {bin_dir}
                "#,
            ),
        ])
        .status()?
        .exit_ok()?)
}
