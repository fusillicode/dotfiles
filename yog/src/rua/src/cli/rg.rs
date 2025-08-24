use crate::cli::Flags;

pub struct CliFlags;

impl Flags for CliFlags {
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
