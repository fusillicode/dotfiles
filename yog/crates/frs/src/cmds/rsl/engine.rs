//! Write mode applies each rule once, then reports against the final immutable snapshot.
//! Report mode only analyzes the supplied snapshot. Earlier plans and findings are never reused.

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use rootcause::report;
use syn::File;

use crate::cmds::rsl::Mode;
use crate::cmds::rsl::crate_roots;
use crate::cmds::rsl::mod_resolution::ModuleGraph;
use crate::cmds::rsl::mod_resolution::SourceFile;
use crate::cmds::rsl::output::FileReport;
use crate::cmds::rsl::output::RuleFinding;
use crate::cmds::rsl::rules::Rule;
use crate::cmds::rsl::rules::SelectedRules;

pub(super) struct FileContext<'ast> {
    pub(super) path: &'ast Path,
    pub(super) source: &'ast str,
    pub(super) file: &'ast File,
    pub(super) modules: &'ast ModuleGraph,
}

pub(super) fn process_paths(
    paths: &[PathBuf],
    rules: &SelectedRules,
    mode: Mode,
) -> rootcause::Result<Vec<FileReport>> {
    let canonical_paths: Vec<_> = paths
        .iter()
        .map(std::fs::canonicalize)
        .collect::<Result<_, _>>()
        .map_err(|error| report!("could not read Rust source").attach(error))?;
    let mut project = ProjectContext::load(&canonical_paths, rules)?;
    let mut visited = HashSet::new();
    let mut processed = Vec::new();

    for (path, canonical) in paths.iter().zip(&canonical_paths) {
        if !visited.insert(canonical) {
            continue;
        }

        let original = project
            .sources
            .get(canonical)
            .cloned()
            .ok_or_else(|| report!("missing Rust source snapshot"))?;
        let source = match mode {
            Mode::Write => rewrite_source(canonical, &original, rules, &mut project)?,
            Mode::Report => original,
        };

        processed.push(ProcessedFile {
            path: path.clone(),
            canonical: canonical.clone(),
            source,
        });
    }

    let mut reports = Vec::new();
    for file in &processed {
        reports.push(FileReport {
            path: file.path.clone(),
            findings: collect_findings(&file.canonical, &file.source, rules, &project.modules)?,
        });
    }

    let changed: Vec<_> = processed
        .iter()
        .filter(|file| project.original_sources.get(&file.canonical) != Some(&file.source))
        .collect();
    if !changed.is_empty() {
        project.ensure_unchanged()?;
    }

    for file in changed {
        // Verify the supplied path still names the source that was analyzed.
        if fs::canonicalize(&file.path).ok().as_ref() != Some(&file.canonical) {
            return Err(
                report!("Rust source path changed during fixing").attach(format!("path={}", file.path.display()))
            );
        }

        fs::write(&file.canonical, file.source.as_bytes()).map_err(|error| {
            report!("could not write Rust source")
                .attach(format!("path={}", file.path.display()))
                .attach(error)
        })?;
    }

    Ok(reports)
}

struct ProcessedFile {
    path: PathBuf,
    canonical: PathBuf,
    source: String,
}

/// Keep the graph and its input snapshots together; topology edits rebuild ancestry before reuse.
struct ProjectContext {
    roots: Vec<PathBuf>,
    sources: HashMap<PathBuf, String>,
    original_sources: HashMap<PathBuf, String>,
    missing_sources: HashSet<PathBuf>,
    modules: ModuleGraph,
}

impl ProjectContext {
    fn load(paths: &[PathBuf], rules: &SelectedRules) -> rootcause::Result<Self> {
        let mut sources = HashMap::new();

        for path in paths {
            sources.insert(
                path.clone(),
                fs::read_to_string(path).map_err(|error| {
                    report!("could not read Rust source")
                        .attach(format!("path={}", path.display()))
                        .attach(error)
                })?,
            );
        }

        let discovery = if rules.iter().any(Rule::needs_module_context) {
            crate_roots::discover(paths)?
        } else {
            crate_roots::CrateRoots::default()
        };
        let mut missing_sources = HashSet::new();
        let modules = discover_modules(&discovery.roots, &mut sources, &mut missing_sources)?;

        let mut original_sources = sources.clone();
        for manifest in discovery.manifests {
            let text = fs::read_to_string(&manifest)
                .map_err(|error| report!("could not read Cargo manifest").attach(error))?;
            original_sources.insert(manifest, text);
        }

        Ok(Self {
            roots: discovery.roots,
            sources,
            original_sources,
            missing_sources,
            modules,
        })
    }

    fn update(&mut self, path: &Path, source: &str, parsed: &File) -> rootcause::Result<()> {
        let topology_changed = !self.modules.matches_structure(path, parsed);
        self.sources.insert(path.to_owned(), source.to_owned());

        if topology_changed {
            self.modules = discover_modules(&self.roots, &mut self.sources, &mut self.missing_sources)?;

            for (path, source) in &self.sources {
                self.original_sources
                    .entry(path.clone())
                    .or_insert_with(|| source.clone());
            }
        }

        Ok(())
    }

    fn ensure_unchanged(&self) -> rootcause::Result<()> {
        for (path, original) in &self.original_sources {
            if fs::read_to_string(path).ok().as_deref() != Some(original.as_str()) {
                return Err(report!("Rust project changed during fixing").attach(format!("path={}", path.display())));
            }
        }

        for path in &self.missing_sources {
            if path
                .try_exists()
                .map_err(|error| report!("could not check module candidate").attach(error))?
            {
                return Err(
                    report!("Rust module candidate appeared during fixing").attach(format!("path={}", path.display()))
                );
            }
        }

        Ok(())
    }
}

fn discover_modules(
    roots: &[PathBuf],
    sources: &mut HashMap<PathBuf, String>,
    missing: &mut HashSet<PathBuf>,
) -> rootcause::Result<ModuleGraph> {
    ModuleGraph::discover(roots, |path| {
        let canonical = fs::canonicalize(path).inspect_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                missing.insert(path.to_owned());
            }
        })?;

        let text = if let Some(source) = sources.get(&canonical) {
            source.clone()
        } else {
            let source = fs::read_to_string(&canonical)?;
            sources.insert(canonical.clone(), source.clone());
            source
        };

        Ok(SourceFile { path: canonical, text })
    })
    .map_err(|error| report!("could not discover Rust modules").attach(error))
}

fn rewrite_source(
    path: &Path,
    source: &str,
    rules: &SelectedRules,
    project: &mut ProjectContext,
) -> rootcause::Result<String> {
    let mut current = source.to_owned();

    for rule in rules.iter() {
        let file = parse_source(path, &current)?;
        let ctx = FileContext {
            path,
            source: &current,
            file: &file,
            modules: &project.modules,
        };
        let plan = rule.analyze(&ctx)?;
        let fixed = plan.apply_edits()?;
        let parsed = parse_source(path, &fixed)?;
        project.update(path, &fixed, &parsed)?;

        current = fixed;
    }

    Ok(current)
}

fn collect_findings(
    path: &Path,
    source: &str,
    rules: &SelectedRules,
    modules: &ModuleGraph,
) -> rootcause::Result<Vec<RuleFinding>> {
    let file = parse_source(path, source)?;
    let ctx = FileContext {
        path,
        source,
        file: &file,
        modules,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_context_when_module_declarations_change_rebuilds_ancestry() {
        let (_directory, root, mut project) = project();
        let child = root.with_file_name("child.rs");

        assert_eq!(project.modules.anchor(&child, &[], 0), Ok("crate::child".to_owned()));

        let source = r#"
            #[path = "child.rs"]
            mod renamed;
        "#;
        let parsed = syn::parse_file(source).unwrap();
        project.update(&root, source, &parsed).unwrap();

        assert_eq!(project.modules.anchor(&child, &[], 0), Ok("crate::renamed".to_owned()));
        project.ensure_unchanged().unwrap();
    }

    #[rstest::rstest]
    #[case::changed_source("lib.rs", "Rust project changed during fixing")]
    #[case::appearing_candidate("child/mod.rs", "Rust module candidate appeared during fixing")]
    fn test_project_context_when_consulted_inputs_change_rejects_writes(#[case] filename: &str, #[case] message: &str) {
        let (_directory, root, project) = project();
        let changed = root.parent().unwrap().join(filename);
        std::fs::create_dir_all(changed.parent().unwrap()).unwrap();
        std::fs::write(&changed, "").unwrap();

        let error = project.ensure_unchanged().unwrap_err();

        // Reports include source frames; assert the cause and affected input independently of rendering.
        let diagnostic = error.to_string();

        assert!(diagnostic.contains(message), "{diagnostic}");
        assert!(
            diagnostic.contains(&format!("path={}", changed.display())),
            "{diagnostic}"
        );
    }

    fn project() -> (tempfile::TempDir, PathBuf, ProjectContext) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("Cargo.toml"),
            r#"
            [package]
            name = "module-project-fixture"
            version = "0.1.0"
            edition = "2024"
            [lib]
            path = "lib.rs"
            [workspace]
        "#,
        )
        .unwrap();
        let root = directory.path().join("lib.rs");
        std::fs::write(
            &root,
            r"
            mod child;
        ",
        )
        .unwrap();
        std::fs::write(directory.path().join("child.rs"), "").unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let rules = SelectedRules::try_from(vec!["path-anchoring".to_owned()]).unwrap();
        let project = ProjectContext::load(std::slice::from_ref(&root), &rules).unwrap();

        (directory, root, project)
    }
}
