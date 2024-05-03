use crate::utils::system::silent_cmd;

pub fn install(bin_dir: &str) -> anyhow::Result<()> {
    // Installing with `cargo` because of:
    // 1. no particular requirements
    // 2. https://github.com/tamasfe/taplo/issues/542
    silent_cmd("cargo")
        .args([
            "install",
            "taplo-cli",
            "--force",
            "--all-features",
            "--root",
            // `--root` automatically append `bin` 🥲
            bin_dir.trim_end_matches("bin"),
        ])
        .status()?;

    Ok(())
}
