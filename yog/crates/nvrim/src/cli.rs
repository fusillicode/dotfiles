//! CLI flag generation helpers for search tools (`fd`, `rg`).
//!
//! Centralizes glob blacklist + base flags for Rust and plugin-facing helpers.

mod fd;
mod rg;

/// A list of glob patterns to exclude from searches.
pub const GLOB_BLACKLIST: [&str; 7] = [
    "**/.git/*",
    "**/target/*",
    "**/target-*/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

/// Trait for generating CLI flags for search tools.
pub trait CliFlags {
    /// Returns the base flags for the CLI tool.
    fn base_flags() -> Vec<&'static str>;

    /// Converts a glob pattern to a CLI flag.
    fn glob_flag(glob: &str) -> String;

    /// Generates the complete list of CLI flags.
    fn get((): ()) -> Vec<String> {
        Self::base_flags()
            .into_iter()
            .map(Into::into)
            .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag))
            .collect::<Vec<_>>()
    }
}

/// Returns the `fd` flags used by `fzf-lua` file pickers.
pub fn get_fd_flags((): ()) -> Vec<String> {
    fd::FdCliFlags::get(())
}

/// Returns the `rg` flags used by `fzf-lua` grep pickers.
pub fn get_rg_flags((): ()) -> Vec<String> {
    rg::RgCliFlags::get(())
}
