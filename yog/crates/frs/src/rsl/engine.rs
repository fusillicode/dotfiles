//! Rule engine for `frs rsl`.

use std::path::Path;
use std::path::PathBuf;

use rootcause::report;

pub struct FileContext<'ast> {
    pub path: &'ast Path,
    pub file: &'ast syn::File,
}

pub(super) fn check_paths(paths: &[PathBuf]) -> rootcause::Result<Vec<serde_json::Value>> {
    let mut violations = Vec::new();

    for path in paths {
        let source = std::fs::read_to_string(path).map_err(|error| {
            report!("could not read Rust source: {error}").attach(format!("path={}", path.display()))
        })?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            report!("could not parse Rust source: {error}").attach(format!("path={}", path.display()))
        })?;

        let file_violations = crate::rsl::rules::check(&FileContext { path, file: &syntax })?;

        violations.extend(file_violations);
    }

    Ok(violations)
}
