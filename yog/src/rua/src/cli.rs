use mlua::prelude::*;

pub mod fd;
pub mod rg;

/// Type alias for Lua functions that take an optional string argument and return a result.
///
/// This represents a boxed closure that can be called from Lua with an optional
/// [LuaString] parameter and returns a result of type `O`. The function is tied
/// to the lifetime of the Lua state.
type ArityOneLuaFunction<'a, O> = Box<dyn Fn(&Lua, Option<LuaString>) -> anyhow::Result<O> + 'a>;

/// Common glob patterns to exclude from file searches.
///
/// These patterns represent directories that typically contain generated files,
/// dependencies, or version control data that should not be included in file
/// searches or operations. They are used by both fd and ripgrep integrations.
pub const GLOB_BLACKLIST: [&str; 6] = [
    "**/.git/*",
    "**/target/*",
    "**/_build/*",
    "**/deps/*",
    "**/.elixir_ls/*",
    "**/node_modules/*",
];

/// Trait for generating CLI flags for file search tools.
///
/// This trait provides a standardized interface for generating command-line flags
/// for tools like fd and ripgrep. It combines base flags specific to each tool
/// with common glob patterns to exclude unwanted directories.
///
/// Implementors should provide tool-specific base flags and glob flag formatting.
pub trait Flags {
    /// Returns the base flags for the CLI tool.
    ///
    /// These are the fundamental flags that every invocation of the tool should include,
    /// such as flags for color output, hidden file handling, etc.
    ///
    /// # Returns
    ///
    /// A vector of static string slices representing the base flags.
    fn base_flags() -> Vec<&'static str>;

    /// Returns the glob flag for the given pattern.
    ///
    /// This method formats a glob pattern into the appropriate flag format
    /// for the specific CLI tool (e.g., `--exclude` for fd, `--glob` for ripgrep).
    ///
    /// # Arguments
    ///
    /// * `glob` - The glob pattern to format as a flag
    ///
    /// # Returns
    ///
    /// A string containing the formatted glob flag.
    fn glob_flag(glob: &str) -> String;

    /// Returns a Lua function that provides the combined flags.
    ///
    /// This method creates a Lua function that returns a table containing all
    /// the flags (base flags + glob exclusions) that should be used when
    /// invoking the CLI tool from Lua scripts.
    ///
    /// # Returns
    ///
    /// A boxed function that can be called from Lua to get the flags table.
    fn get(&self) -> ArityOneLuaFunction<'_, LuaTable> {
        Box::new(|lua: &Lua, _: Option<LuaString>| {
            Ok(lua.create_sequence_from(
                Self::base_flags()
                    .into_iter()
                    .map(Into::into)
                    .chain(GLOB_BLACKLIST.into_iter().map(Self::glob_flag)),
            )?)
        })
    }
}
