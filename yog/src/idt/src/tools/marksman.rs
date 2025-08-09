use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct Marksman {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Marksman {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        crate::downloaders::curl::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dest_dir, self.bin_name())),
        )?;

        Ok(None)
    }
}
