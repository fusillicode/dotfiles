//! Rule engine for `frs rsl`.

use std::path::Path;
use std::path::PathBuf;

use rootcause::report;

pub struct FileContext<'ast> {
    pub path: &'ast Path,
    pub file: &'ast syn::File,
}

pub(super) fn check_paths(paths: &[PathBuf]) -> rootcause::Result<Vec<Box<dyn crate::rsl::rules::RuleViolation>>> {
    let mut violations = Vec::new();

    for path in paths {
        let source = std::fs::read_to_string(path).map_err(|error| {
            report!("could not read Rust source")
                .attach(format!("path={}", path.display()))
                .attach(format!("error={error}"))
        })?;
        let syntax = syn::parse_file(&source).map_err(|error| {
            report!("could not parse Rust source")
                .attach(format!("path={}", path.display()))
                .attach(format!("error={error}"))
        })?;

        let file_violations = crate::rsl::rules::check(&FileContext { path, file: &syntax });

        violations.extend(file_violations);
    }

    Ok(violations)
}
