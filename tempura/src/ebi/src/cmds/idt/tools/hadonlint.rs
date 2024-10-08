use crate::cmds::idt::curl_install::OutputOption;
use crate::cmds::idt::tools::Installer;

pub struct HadolintInstaller {
    pub bin_dir: String,
}

impl Installer for HadolintInstaller {
    fn bin(&self) -> &'static str {
        "hadolint"
    }

    fn install(&self) -> anyhow::Result<()> {
        crate::cmds::idt::curl_install::run(
            &format!(
                "https://github.com/{0}/{0}/releases/latest/download/{0}-Darwin-x86_64",
                self.bin()
            ),
            OutputOption::WriteTo(&format!("{}/{}", self.bin_dir, self.bin())),
        )
    }
}
