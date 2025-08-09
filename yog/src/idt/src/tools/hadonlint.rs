use crate::ToolInstaller;
use crate::downloaders::curl::OutputOption;
use crate::tools::NeedSymlink;

pub struct Hadolint {
    pub bin_dest_dir: String,
}

impl ToolInstaller for Hadolint {
    fn bin_name(&self) -> &'static str {
        "hadolint"
    }

    fn download(&self) -> color_eyre::Result<NeedSymlink> {
        let bin_src = crate::downloaders::curl::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-Darwin-x86_64",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dest_dir, self.bin_name())),
        )?;

        Ok(NeedSymlink::No {
            src: bin_src.into(),
        })
    }
}
