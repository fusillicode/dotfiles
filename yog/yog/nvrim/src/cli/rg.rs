use crate::cli::CliFlags;

/// CLI flags for the `rg` command.
pub struct RgCliFlags;

impl CliFlags for RgCliFlags {
    /// Returns the base flags for the `rg` command.
    fn base_flags() -> Vec<&'static str> {
        vec![
            "--color never",
            "--column",
            "--hidden",
            "--line-number",
            "--no-heading",
            "--smart-case",
            "--with-filename",
        ]
    }

    /// Returns the glob flag for the given pattern.
    fn glob_flag(glob: &str) -> String {
        format!("--glob !'{glob}'")
    }
}
