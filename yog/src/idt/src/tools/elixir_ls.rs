use std::process::Command;

use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct ElixirLs {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for ElixirLs {
    fn bin_name(&self) -> &'static str {
        "elixir-ls"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let repo = format!("elixir-lsp/{}", self.bin_name());
        let dev_tools_repo_dir = format!("{}/{}", self.dev_tools_dir, self.bin_name());
        let latest_release = utils::github::get_latest_release(&repo)?;
        std::fs::create_dir_all(&dev_tools_repo_dir)?;

        crate::downloaders::curl::run(
            &format!(
                "https://github.com/{repo}/releases/download/{latest_release}/{}-{latest_release}.zip",
                self.bin_name()
            ),
            OutputOption::PipeInto(
                Command::new("tar").args(["-xz", "-C"]),
                dev_tools_repo_dir.clone(),
            ),
        )?;

        Ok(NeedSymlink::Yes {
            src: format!("{dev_tools_repo_dir}/language_server.sh").into(),
            dest: format!("{}/{}", self.bin_dest_dir, self.bin_name()).into(),
        })
    }
}
