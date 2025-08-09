use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;

pub struct HelmLs {
    pub bin_dest_dir: String,
}

impl ToolInstaller for HelmLs {
    fn bin_name(&self) -> &'static str {
        "helm_ls"
    }

    fn download(&self) -> color_eyre::Result<()> {
        crate::downloaders::curl::run(
            &format!(
                "https://github.com/mrjosh/helm-ls/releases/latest/download/{}_darwin_amd64",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dest_dir, self.bin_name())),
        )?;

        Ok(())
    }
}
