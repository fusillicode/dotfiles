const HELP_ARG: &str = "--help";

/// Abstraction over command-line argument collections.
pub trait Args<T> {
    /// Checks if the help flag (`--help`) is present.
    fn has_help(&self) -> bool;

    /// Returns a copy of all arguments.
    fn all(&self) -> Vec<T>;
}

impl<T: AsRef<str> + Clone> Args<T> for Vec<T> {
    fn has_help(&self) -> bool {
        self.iter().any(|arg| arg.as_ref() == HELP_ARG)
    }

    fn all(&self) -> Self {
        self.clone()
    }
}

impl Args<String> for pico_args::Arguments {
    fn has_help(&self) -> bool {
        self.clone().contains(HELP_ARG)
    }

    fn all(&self) -> Vec<String> {
        self.clone()
            .finish()
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
}

/// Retrieves command-line arguments excluding the program name, returning them as a [`Vec`] of [`String`].
pub fn get() -> Vec<String> {
    let mut args = std::env::args();
    args.next();
    args.collect::<Vec<String>>()
}
