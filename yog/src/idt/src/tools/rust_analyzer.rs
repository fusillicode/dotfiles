use std::process::Command;

use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct RustAnalyzer {
    pub bin_dest_dir: String,
}

impl ToolInstaller for RustAnalyzer {
    fn bin_name(&self) -> &'static str {
        "rust-analyzer"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let bin_src = crate::downloaders::curl::run(
            &format!(
                "https://github.com/rust-lang/{0}/releases/download/nightly/{0}-aarch64-apple-darwin.gz",
                self.bin_name()
            ),
            OutputOption::UnpackVia(
                Box::new(Command::new("zcat")),
                &format!("{}/{}", self.bin_dest_dir, self.bin_name()),
            ),
        )?;

        Ok(NeedSymlink::No {
            src: bin_src.into(),
        })
    }
}
