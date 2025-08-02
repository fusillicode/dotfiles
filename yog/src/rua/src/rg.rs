use crate::cli::Flags;

pub struct CliFlagsImpl;

impl Flags for CliFlagsImpl {
    fn base_flags(&self) -> Vec<&str> {
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

    fn format_glob(&self, glob: &str) -> String {
        format!("--glob !'{glob}'")
    }
}
