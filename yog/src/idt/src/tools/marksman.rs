use crate::ToolInstaller;
use crate::installers::curl_install::OutputOption;

pub struct Marksman {
    pub bin_target_dir: String,
}

impl ToolInstaller for Marksman {
    fn bin_name(&self) -> &'static str {
        "marksman"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/artempyanykh/{0}/releases/latest/download/{0}-macos",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_target_dir, self.bin_name())),
        )
    }
}
