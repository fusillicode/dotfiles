//! Parse each rule's current snapshot, validate the result, then write each supplied file once.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::report;
use syn::File;

use super::rules::SelectedRules;
use super::source::SourceEditor;

pub(super) struct FileContext<'ast> {
    pub(super) path: &'ast Path,
    pub(super) source: &'ast str,
    pub(super) file: &'ast File,
}

pub(super) fn fix_paths(paths: &[PathBuf], rules: &SelectedRules) -> rootcause::Result<Vec<String>> {
    let mut visited = HashSet::new();
    let mut skips = Vec::new();

    for path in paths {
        let canonical = std::fs::canonicalize(path).map_err(|error| {
            report!("could not read Rust source")
                .attach(format!("path={}", path.display()))
                .attach(format!("error={error}"))
        })?;

        if !visited.insert(canonical) {
            continue;
        }

        let source = std::fs::read_to_string(path).map_err(|error| {
            report!("could not read Rust source")
                .attach(format!("path={}", path.display()))
                .attach(format!("error={error}"))
        })?;

        let (fixed, file_skips) = fix_source(path, &source, rules)?;

        if fixed != source {
            // A concurrent editor must not lose changes made after our snapshot.
            if std::fs::read_to_string(path).ok().as_deref() != Some(source.as_str()) {
                return Err(report!("Rust source changed during fixing").attach(format!("path={}", path.display())));
            }

            std::fs::write(path, fixed).map_err(|error| {
                report!("could not write Rust source")
                    .attach(format!("path={}", path.display()))
                    .attach(format!("error={error}"))
            })?;
        }

        skips.extend(file_skips.into_iter().map(|skip| format!("{}: {skip}", path.display())));
    }

    Ok(skips)
}

pub(super) fn fix_source(path: &Path, source: &str, rules: &SelectedRules) -> rootcause::Result<(String, Vec<String>)> {
    let mut current = source.to_owned();
    let mut skips = Vec::new();

    for rule in rules.iter() {
        let file = parse_source(path, &current)?;
        let ctx = FileContext {
            path,
            source: &current,
            file: &file,
        };
        let mut editor = SourceEditor::new(&current, rule.id());

        rule.fix(&ctx, &mut editor)?;
        let (fixed, rule_skips) = editor.finish()?;
        parse_source(path, &fixed)?;

        current = fixed;
        skips.extend(rule_skips);
    }

    Ok((current, skips))
}

fn parse_source(path: &Path, source: &str) -> rootcause::Result<File> {
    syn::parse_file(source).map_err(|error| {
        report!("could not parse Rust source")
            .attach(format!("path={}", path.display()))
            .attach(format!("error={error}"))
    })
}
