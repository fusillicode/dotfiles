//! Provide minimal TUI selection & prompt helpers built on [`skim`].
//!
//! Offer uniform, cancellable single / multi select prompts with fuzzy filtering and helpers
//! to derive a value from CLI args or fallback to an interactive selector.

use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
pub use interactive::*;

#[cfg(not(target_arch = "wasm32"))]
pub mod git_branch;
#[cfg(not(target_arch = "wasm32"))]
mod interactive;

pub fn display_fixed_width(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = normalized.chars().collect();

    if chars.len() <= max_chars {
        return normalized;
    }

    if max_chars == 0 {
        return String::new();
    }

    if max_chars == 1 {
        return "…".to_owned();
    }

    let mut trimmed: String = chars.into_iter().take(max_chars.saturating_sub(1)).collect();
    trimmed.push('…');
    trimmed
}

pub fn short_path(path: &Path, home: &Path) -> String {
    if home != Path::new("/") {
        if path == home {
            return "~".into();
        }
        if let Ok(rel) = path.strip_prefix(home) {
            let names = path_dir_names(rel);
            return if names.is_empty() {
                "~".into()
            } else {
                format!("~/{}", abbrev_path_dirs(&names))
            };
        }
    }

    let names = path_dir_names(path);
    if names.is_empty() {
        "/".into()
    } else {
        format!("/{}", abbrev_path_dirs(&names))
    }
}

fn path_dir_names(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect()
}

fn abbrev_path_dirs(names: &[String]) -> String {
    match names.len() {
        0 => String::new(),
        1 => names.first().cloned().unwrap_or_default(),
        total => {
            let mut out = String::new();
            for (idx, name) in names.iter().enumerate() {
                if idx > 0 {
                    out.push('/');
                }
                let is_last = idx == total.saturating_sub(1);
                if is_last {
                    out.push_str(name);
                } else {
                    out.push(name.chars().next().unwrap_or('·'));
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[rstest::rstest]
    #[case("hello world", 20, "hello world")]
    #[case("abcdefghijklmnopqrstuvwxyz", 5, "abcd…")]
    #[case("abc", 1, "…")]
    #[case("abc", 0, "")]
    fn display_fixed_width_trims_as_expected(#[case] value: &str, #[case] max_chars: usize, #[case] expected: &str) {
        pretty_assertions::assert_eq!(display_fixed_width(value, max_chars), expected);
    }

    #[test]
    fn test_short_path_under_home_abbreviates_parent_directories() {
        let home = Path::new("/home/user");

        pretty_assertions::assert_eq!(
            short_path(Path::new("/home/user/src/pkg/myproject"), home),
            "~/s/p/myproject"
        );
    }

    #[test]
    fn test_short_path_many_dirs_abbreviates_all_but_last() {
        let home = Path::new("/home/user");

        pretty_assertions::assert_eq!(
            short_path(Path::new("/home/user/one/two/three/four/five"), home),
            "~/o/t/t/f/five"
        );
    }

    #[test]
    fn test_short_path_outside_home_renders_absolute_abbrev() {
        let home = Path::new("/home/user");

        pretty_assertions::assert_eq!(short_path(Path::new("/opt/pkg/foo"), home), "/o/p/foo");
    }
}
