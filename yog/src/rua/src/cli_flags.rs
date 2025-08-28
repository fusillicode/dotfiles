pub mod fd;
pub mod rg;

/// A list of glob patterns to exclude from searches.
pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
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

    /// Returns a closure that generates the complete list of CLI flags.
    fn get(&self) -> Box<dyn Fn(()) -> Vec<String>> {
        Box::new(|_| {
            Self::base_flags()
                .into_iter()
                .map(Into::into)
                .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag))
                .collect::<Vec<_>>()
        })
    }
}
