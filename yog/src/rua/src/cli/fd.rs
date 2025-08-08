use crate::cli::Flags;

pub struct CliFlags;

impl Flags for CliFlags {
    fn base_flags() -> Vec<&'static str> {
        vec![
            "--color never",
            "--follow",
            "--hidden",
            "--no-ignore-vcs",
            "--type f",
        ]
    }

    fn glob_flag(glob: &str) -> String {
        format!("--exclude '{glob}'")
    }
}
