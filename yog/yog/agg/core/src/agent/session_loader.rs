use std::path::Path;

use rootcause::prelude::ResultExt as _;

pub mod claude;
pub mod codex;
pub mod cursor;

fn file_updated_at(path: &Path) -> rootcause::Result<Option<chrono::DateTime<chrono::Utc>>> {
    let modified = std::fs::metadata(path)
        .context("failed to read session metadata")
        .attach_with(|| format!("path={}", path.display()))?
        .modified()
        .context("failed to read session modified time")
        .attach_with(|| format!("path={}", path.display()))?;
    Ok(Some(chrono::DateTime::<chrono::Utc>::from(modified)))
}
