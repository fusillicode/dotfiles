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

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::empty_vec(vec![], false)]
    #[case::no_help(vec!["arg1", "arg2"], false)]
    #[case::has_help(vec!["--help"], true)]
    #[case::help_among_others(vec!["foo", "--help", "bar"], true)]
    fn has_help_for_vec_returns_expected(#[case] args: Vec<&str>, #[case] expected: bool) {
        pretty_assertions::assert_eq!(args.has_help(), expected);
    }

    #[rstest::rstest]
    #[case::empty_vec(Vec::<String>::new(), Vec::<String>::new())]
    #[case::clones_all(vec!["a".to_owned(), "b".to_owned()], vec!["a".to_owned(), "b".to_owned()])]
    fn all_for_vec_returns_clone(#[case] args: Vec<String>, #[case] expected: Vec<String>) {
        pretty_assertions::assert_eq!(args.all(), expected);
    }
}
