use crate::cli_flags::CliFlags;

/// CLI flags for the fd command.
pub struct FdCliFlags;

impl CliFlags for FdCliFlags {
    /// Returns the base flags for the fd command.
    fn base_flags() -> Vec<&'static str> {
        vec!["--color never", "--follow", "--hidden", "--no-ignore-vcs", "--type f"]
    }

    /// Returns the exclude flag for the given glob pattern.
    fn glob_flag(glob: &str) -> String {
        format!("--exclude '{glob}'")
    }
}
