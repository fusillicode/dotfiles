use crate::installers::curl_install::OutputOption;
use crate::Installer;

pub struct Ollama {
    pub bin_dir: String,
}

impl Installer for Ollama {
    fn bin_name(&self) -> &'static str {
        "ollama"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::installers::curl_install::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-darwin",
                self.bin_name()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin_name())),
        )
    }
}
