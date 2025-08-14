use std::path::PathBuf;

use color_eyre::eyre::eyre;

use crate::Installer;

pub struct VsCodeLangServers {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for VsCodeLangServers {
    fn bin_name(&self) -> &'static str {
        "vscode-langservers-extracted"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir =
            crate::downloaders::npm::run(&self.dev_tools_dir, self.bin_name(), &[self.bin_name()])?;

        // FIXME: link and chmod glob
        let link_dir = PathBuf::from(&self.bin_dir);
        for target in std::fs::read_dir(target_dir)? {
            let target = target?.path();
            if target.is_file() {
                let target_name = target
                    .file_name()
                    .ok_or_else(|| eyre!("target {target:?} without filename"))?;
                let link_path = link_dir.join(target_name);
                if link_path.exists() {
                    std::fs::remove_file(&link_path)?;
                }
                std::os::unix::fs::symlink(&target, &link_path)?;
                utils::system::chmod_x(&target)?;
            }
        }

        Ok(())
    }
}
