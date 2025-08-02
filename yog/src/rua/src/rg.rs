use crate::cli::Flags;

pub struct CliFlags;

impl Flags for CliFlags {
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

    fn glob_flag(glob: &str) -> String {
        format!("--glob !'{glob}'")
    }
}
