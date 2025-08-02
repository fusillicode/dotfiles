use crate::cli::Flags;

pub struct CliFlagsImpl;

impl Flags for CliFlagsImpl {
    fn base_flags(&self) -> Vec<&str> {
        vec![
            "--color never",
            "--follow",
            "--hidden",
            "--no-ignore-vcs",
            "--type f",
        ]
    }

    fn format_glob(&self, glob: &str) -> String {
        format!("--exclude '{glob}'")
    }
}
