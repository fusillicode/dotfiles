use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre::OptionExt as _;

/// Builds a path starting from the home directory by appending the given parts, returning a [`PathBuf`].
///
/// # Errors
/// - The home directory cannot be determined.
pub fn build_home_path<P: AsRef<Path>>(parts: &[P]) -> color_eyre::Result<PathBuf> {
    let home_path = std::env::home_dir().ok_or_eyre("missing home dir | env=HOME")?;
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
pub fn get_workspace_root() -> color_eyre::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_eyre(format!(
            "cannot get workspace root | manifest_dir={}",
            manifest_dir.display()
        ))?
        .to_path_buf())
}
