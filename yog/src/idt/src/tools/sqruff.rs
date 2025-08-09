use std::process::Command;

use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct Sqruff {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Sqruff {
    fn bin_name(&self) -> &'static str {
        "sqruff"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let bin_src = crate::downloaders::curl::run(
            &format!(
                "https://github.com/quarylabs/{0}/releases/latest/download/{0}-darwin-aarch64.tar.gz",
                self.bin_name()
            ),
            OutputOption::PipeInto(
                Command::new("tar").args(["-xz", "-C"]),
                self.bin_dest_dir.clone(),
            ),
        )?;

        Ok(NeedSymlink::No {
            src: bin_src.into(),
        })
    }
}
