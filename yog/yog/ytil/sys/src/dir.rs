use std::path::Path;
use std::path::PathBuf;

use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;

/// Builds a path starting from the home directory by appending the given parts, returning a [`PathBuf`].
///
/// # Errors
/// - The home directory cannot be determined.
pub fn build_home_path<P: AsRef<Path>>(parts: &[P]) -> rootcause::Result<PathBuf> {
    let home_path = home::home_dir().context("missing home dir").attach("env=HOME")?;
    Ok(build_path(home_path, parts))
}

/// Builds a path by appending multiple parts to a root path.
pub fn build_path<P: AsRef<Path>>(mut root: PathBuf, parts: &[P]) -> PathBuf {
    for part in parts {
        root.push(part);
    }
    root
}

/// Resolve workspace root directory.
///
/// Ascends three levels from this crate's manifest.
///
/// # Errors
/// - Directory traversal fails (unexpected layout).
pub fn get_workspace_root() -> rootcause::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .context(format!(
            "cannot get workspace root | manifest_dir={}",
            manifest_dir.display()
        ))?
        .to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_path_appends_parts_to_root() {
        let root = PathBuf::from("/base");
        let result = build_path(root, &["a", "b", "c"]);
        pretty_assertions::assert_eq!(result, PathBuf::from("/base/a/b/c"));
    }

    #[test]
    fn build_path_with_empty_parts_returns_root() {
        let root = PathBuf::from("/base");
        let result = build_path(root, &[] as &[&str]);
        pretty_assertions::assert_eq!(result, PathBuf::from("/base"));
    }

    #[test]
    fn build_home_path_returns_path_ending_with_parts() {
        assert2::let_assert!(Ok(path) = build_home_path(&[".config", "test"]));
        assert!(path.ends_with(".config/test"), "path={}", path.display());
    }

    #[test]
    fn get_workspace_root_returns_existing_directory() {
        assert2::let_assert!(Ok(root) = get_workspace_root());
        assert!(root.is_dir(), "root={}", root.display());
        // The workspace root should contain Cargo.toml
        assert!(root.join("Cargo.toml").exists(), "root={}", root.display());
    }
}
