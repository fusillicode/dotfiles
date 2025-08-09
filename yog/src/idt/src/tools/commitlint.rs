use crate::ToolInstaller;
use crate::tools::NeedSymlink;

pub struct Commitlint {
    pub dev_tools_dir: String,
    pub bin_dest_dir: String,
}

impl ToolInstaller for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn download(&self) -> color_eyre::Result<Option<NeedSymlink>> {
        let bin_src_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
        )?;

        Ok(Some(NeedSymlink {
            src: format!("{bin_src_dir}/{}", self.bin_name()).into(),
            dest: self.bin_dest_dir.clone().into(),
        }))
    }
}
