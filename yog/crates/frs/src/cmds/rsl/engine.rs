//! Write mode applies each rule once, then reports against the final immutable snapshot.
//! Report mode only analyzes the supplied snapshot. Earlier plans and findings are never reused.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use rootcause::report;
use syn::File;

use super::Mode;
use super::output::FileReport;
use super::output::RuleFinding;
use super::rules::SelectedRules;

pub(super) struct FileContext<'ast> {
    pub(super) path: &'ast Path,
    pub(super) source: &'ast str,
    pub(super) file: &'ast File,
}

pub(super) fn process_paths(
    paths: &[PathBuf],
    rules: &SelectedRules,
    mode: Mode,
) -> rootcause::Result<Vec<FileReport>> {
    let mut visited = HashSet::new();
    let mut reports = Vec::new();

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

        let fixed = match mode {
            Mode::Write => Cow::Owned(rewrite_source(path, &source, rules)?),
            Mode::Report => Cow::Borrowed(source.as_str()),
        };
        let findings = collect_findings(path, &fixed, rules)?;

        if fixed.as_ref() != source {
            // A concurrent editor must not lose changes made after our snapshot.
            if std::fs::read_to_string(path).ok().as_deref() != Some(source.as_str()) {
                return Err(report!("Rust source changed during fixing").attach(format!("path={}", path.display())));
            }

            std::fs::write(path, fixed.as_bytes()).map_err(|error| {
                report!("could not write Rust source")
                    .attach(format!("path={}", path.display()))
                    .attach(format!("error={error}"))
            })?;
        }

        reports.push(FileReport {
            path: path.clone(),
            findings,
        });
    }

    Ok(reports)
}

fn rewrite_source(path: &Path, source: &str, rules: &SelectedRules) -> rootcause::Result<String> {
    let mut current = source.to_owned();

    for rule in rules.iter() {
        let file = parse_source(path, &current)?;
        let ctx = FileContext {
            path,
            source: &current,
            file: &file,
        };
        let plan = rule.analyze(&ctx)?;
        let fixed = plan.apply_edits()?;
        parse_source(path, &fixed)?;

        current = fixed;
    }

    Ok(current)
}

fn collect_findings(path: &Path, source: &str, rules: &SelectedRules) -> rootcause::Result<Vec<RuleFinding>> {
    let file = parse_source(path, source)?;
    let ctx = FileContext {
        path,
        source,
        file: &file,
    };
    let mut findings = Vec::new();

    for rule in rules.iter() {
        let plan = rule.analyze(&ctx)?;
        findings.extend(plan.into_findings().into_iter().map(|finding| RuleFinding {
            rule_id: rule.id(),
            finding,
        }));
    }

    findings.sort_by_key(|finding| (finding.finding.position.line, finding.finding.position.column));
    Ok(findings)
}

fn parse_source(path: &Path, source: &str) -> rootcause::Result<File> {
    syn::parse_file(source).map_err(|error| {
        report!("could not parse Rust source")
            .attach(format!("path={}", path.display()))
            .attach(format!("error={error}"))
    })
}
