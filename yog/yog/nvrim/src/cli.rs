//! CLI flag generation helpers for search tools (`fd`, `rg`).
//!
//! Centralizes glob blacklist + base flags; exposes a dictionary with per‑tool flag builders for Lua.

use nvim_oxi::Dictionary;

mod fd;
mod rg;

/// A list of glob patterns to exclude from searches.
pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

/// [`Dictionary`] of CLI flag generators.
pub fn dict() -> Dictionary {
    dict! {
        "get_fd_flags": fn_from!(fd::FdCliFlags::get),
        "get_rg_flags": fn_from!(rg::RgCliFlags::get),
    }
}

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
