use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;

pub struct OllamaInstaller {
    pub bin_dir: String,
}

impl Installer for OllamaInstaller {
    fn bin(&self) -> &'static str {
        "ollama"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::curl_install::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-darwin",
                self.bin()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin())),
        )
    }
}
