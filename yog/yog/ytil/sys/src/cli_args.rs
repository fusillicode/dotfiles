const HELP_ARG: &str = "--help";

/// Abstraction over command-line argument collections for help detection and access.
///
/// # Type Parameters
/// - `T` The type of individual arguments, typically string-like (e.g., `String`, `&str`).
///
/// # Rationale
/// - Enables polymorphic argument handling without coupling to specific collection types.
/// - Centralizes help flag detection logic for consistency across binaries.
/// - Supports both owned and borrowed argument slices for flexibility.
///
/// # Performance
/// - `has_help` performs a linear scan; suitable for small argument lists (typical CLI usage).
/// - `all` clones arguments; use borrowed types (`T = &str`) to avoid allocation overhead.
pub trait CliArgs<T> {
    /// Checks if the help flag (`--help`) is present in the arguments.
    ///
    /// # Returns
    /// - `true` if `--help` is found; `false` otherwise.
    ///
    /// # Rationale
    /// - Standardized help detection avoids ad-hoc string comparisons in binaries.
    /// - Case-sensitive matching aligns with common CLI conventions.
    fn has_help(&self) -> bool;

    /// Returns a copy of all arguments.
    ///
    /// # Returns
    /// - A vector containing all arguments in their original order.
    ///
    /// # Rationale
    /// - Provides uniform access to arguments regardless of underlying storage.
    /// - Cloning ensures caller ownership; consider `T = &str` for zero-copy variants.
    fn all(&self) -> Vec<T>;
}

impl<T: AsRef<str> + Clone> CliArgs<T> for Vec<T> {
    fn has_help(&self) -> bool {
        self.iter().any(|arg| arg.as_ref() == HELP_ARG)
    }

    fn all(&self) -> Self {
        self.clone()
    }
}

impl CliArgs<String> for pico_args::Arguments {
    fn has_help(&self) -> bool {
        self.clone().contains(HELP_ARG)
    }

    fn all(&self) -> Vec<String> {
        self.clone()
            .finish()
            .into_iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }
}

/// Retrieves command-line arguments excluding the program name, returning them as a [`Vec`] of [`String`].
pub fn get() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}
