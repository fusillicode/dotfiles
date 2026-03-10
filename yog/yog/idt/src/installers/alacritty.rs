use std::path::Path;

use rootcause::prelude::ResultExt as _;
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

        crate::installers::install_macos_app(&app, self.bin_dir, self.bin_name())?;

        // Alacritty sets TERM=alacritty, but programs need a matching terminfo entry to
        // know the terminal's capabilities. Without it the system falls back to TERM=dumb,
        // which breaks tools that depend on a capable terminal (e.g. starship, neovim).
        // The app bundle ships pre-compiled entries; copy them to ~/.terminfo/ (no sudo,
        // bypasses macOS SIP on /usr/share, avoids tic compatibility issues).
        let home = std::env::var("HOME").context("error reading HOME env var")?;
        let terminfo_dest = Path::new(&home).join(".terminfo").join("61");
        std::fs::create_dir_all(&terminfo_dest)
            .context("error creating ~/.terminfo/61")
            .attach_with(|| format!("path={}", terminfo_dest.display()))?;

        let bundled = app.join("Contents").join("Resources").join("61");
        for entry in &["alacritty", "alacritty-direct"] {
            let src = bundled.join(entry);
            let dst = terminfo_dest.join(entry);
            std::fs::copy(&src, &dst)
                .context("error copying terminfo entry")
                .attach_with(|| format!("src={}", src.display()))
                .attach_with(|| format!("dst={}", dst.display()))?;
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
