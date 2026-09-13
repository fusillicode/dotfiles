//! Rule engine for the `frs rsl` command.

use std::path::Path;
use std::path::PathBuf;

use rayon::prelude::*;
use rootcause::report;

use crate::cmds::rsl::rules::RuleViolation;

pub struct FileContext<'ast> {
    pub path: &'ast Path,
    pub file: &'ast syn::File,
    pub(super) module_item_lists: Vec<Vec<crate::cmds::rsl::ast::ModuleItem<'ast>>>,
}

pub(super) fn check_paths(paths: &[PathBuf]) -> rootcause::Result<Vec<Box<dyn RuleViolation>>> {
    // Collect indexed results before propagating errors to preserve input order.
    let file_results: Vec<_> = paths.par_iter().map(|path| self::check_path(path)).collect();
    let mut violations = Vec::new();

    for file_result in file_results {
        violations.extend(file_result?);
    }

    Ok(violations)
}

fn check_path(path: &Path) -> rootcause::Result<Vec<Box<dyn RuleViolation>>> {
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
    let module_item_lists = crate::cmds::rsl::ast::module_item_lists(&syntax);

    Ok(crate::cmds::rsl::rules::check(&FileContext {
        path,
        file: &syntax,
        module_item_lists,
    }))
}
