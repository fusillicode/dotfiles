use crate::Installer;
use crate::downloaders::curl::CurlDownloaderOption;

pub struct LuaLanguageServer {
    pub dev_tools_dir: String,
}

impl Installer for LuaLanguageServer {
    fn bin_name(&self) -> &'static str {
        "lua-language-server"
    }

    fn download(&self) -> color_eyre::Result<()> {
        // No `bin` link as it requires some local stuff so, leave the garbage in `dev-tools` and configure the LSP to point to
        // the `bin` there.
        let repo = format!("LuaLS/{}", self.bin_name());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}-darwin-arm64.tar.gz",
                self.bin_name()
            ),
            CurlDownloaderOption::PipeIntoTar {
                dest_dir: &dev_tools_repo_dir,
                dest_name: None,
            },
        )?;

        Ok(())
    }
}
