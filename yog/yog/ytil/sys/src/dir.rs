use std::path::Path;
use std::path::PathBuf;

use rootcause::option_ext::OptionExt;
#[cfg(not(target_arch = "wasm32"))]
use rootcause::prelude::ResultExt;

/// Builds a path starting from the home directory by appending the given parts, returning a [`PathBuf`].
///
/// # Errors
/// - The home directory cannot be determined.
#[cfg(not(target_arch = "wasm32"))]
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
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_build_path_appends_parts_to_root() {
        let root = PathBuf::from("/base");
        let result = build_path(root, &["a", "b", "c"]);
        assert_that!(result, eq(PathBuf::from("/base/a/b/c")));
    }

    #[test]
    fn test_build_path_with_empty_parts_returns_root() {
        let root = PathBuf::from("/base");
        let result = build_path(root, &[] as &[&str]);
        assert_that!(result, eq(PathBuf::from("/base")));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_build_home_path_returns_path_ending_with_parts() {
        assert_that!(
            build_home_path(&[".config", "test"]),
            ok(predicate(|path: &PathBuf| path.ends_with(".config/test"))
                .with_description("ends with .config/test", "does not end with .config/test"))
        );
    }

    #[test]
    fn test_get_workspace_root_returns_existing_directory() {
        let root_result = get_workspace_root();
        assert_that!(root_result.as_ref().map(|_| ()), ok(eq(())));
        let root = root_result.expect("workspace root should resolve");
        assert_that!(root.is_dir(), eq(true));
        // The workspace root should contain Cargo.toml
        assert_that!(root.join("Cargo.toml").exists(), eq(true));
    }
}
