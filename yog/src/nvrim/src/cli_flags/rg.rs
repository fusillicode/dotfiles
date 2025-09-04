use crate::cli_flags::CliFlags;

/// CLI flags for the ripgrep command.
pub struct RgCliFlags;

impl CliFlags for RgCliFlags {
    /// Returns the base flags for the ripgrep command.
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
