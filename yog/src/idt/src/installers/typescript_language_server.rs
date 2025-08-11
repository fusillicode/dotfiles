use crate::Installer;

pub struct TypescriptLanguageServer {
    pub dev_tools_dir: String,
    pub bin_dir: String,
}

impl Installer for TypescriptLanguageServer {
    fn bin_name(&self) -> &'static str {
        "typescript-language-server"
    }

    fn install(&self) -> color_eyre::Result<()> {
        crate::downloaders::npm::run(
            &self.dev_tools_dir,
            self.bin_name(),
            &[self.bin_name(), "typescript"],
            &self.bin_dir,
            self.bin_name(),
        )?;

        Ok(())
    }
}
