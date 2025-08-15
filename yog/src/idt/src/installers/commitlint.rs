use crate::Installer;

pub struct Commitlint {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for Commitlint {
    fn bin_name(&self) -> &'static str {
        "commitlint"
    }

    fn install(&self) -> color_eyre::Result<()> {
        let target_dir = crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[
                &format!("@{}/cli", self.bin_name()),
                &format!("@{}/config-conventional", self.bin_name()),
            ],
        )?;

        let target = format!("{target_dir}/{}", self.bin_name());
        utils::system::ln_sf(&target, &format!("{}/{}", self.bin_dir, self.bin_name()))?;
        utils::system::chmod_x(target)?;

        Ok(())
    }

    fn check_args(&self) -> Option<&[&str]> {
        Some(&["--version"])
    }
}
