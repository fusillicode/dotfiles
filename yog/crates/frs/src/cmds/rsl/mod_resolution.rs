//! Discover module ancestry from explicit crate roots and a source reader, without compiling Rust.
//!
//! A source file can occur in several modules. Keep those occurrences separate and only answer an
//! anchoring query when they all agree. Locations use inline module names rather than byte offsets,
//! so ordinary edits and item reordering do not invalidate ancestry.
//!
//! Like the rewriting rules, discovery trusts explicit declarations: arbitrary macro
//! and attribute expansions are assumed not to relocate them or add hidden occurrences.
//! Literal includes are still followed; computed include paths remain unresolved.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use syn::File;

use crate::cmds::rsl::mod_resolution::declarations::ModuleStructure;
use crate::cmds::rsl::mod_resolution::discovery::ModulePlacement;
use crate::cmds::rsl::mod_resolution::discovery::ModuleResolution;

mod declarations;
mod discovery;

/// The reader supplies the canonical identity as well as the source snapshot.
pub(super) struct SourceFile {
    pub(super) path: PathBuf,
    pub(super) text: String,
}

#[derive(Default)]
pub(super) struct ModuleGraph {
    modules: Vec<Module>,
    unresolved_files: HashSet<PathBuf>,
    unexpanded_roots: HashSet<usize>,
    structures: HashMap<PathBuf, Vec<ModuleStructure>>,
}

impl ModuleGraph {
    pub(super) fn discover(roots: &[PathBuf], read: impl FnMut(&Path) -> io::Result<SourceFile>) -> io::Result<Self> {
        let discovery = discovery::resolve_roots(roots, read)?;

        let mut graph = Self {
            structures: discovery.structures,
            ..Self::default()
        };
        let mut pending: Vec<(Option<ModuleParent>, ModuleResolution)> =
            discovery.roots.into_iter().rev().map(|root| (None, root)).collect();

        while let Some((parent, resolution)) = pending.pop() {
            match resolution {
                ModuleResolution::Module { description, children } => {
                    let id = graph.modules.len();
                    let crate_root = parent.map_or(id, |parent| parent.crate_root);
                    let addressable = description.placement.is_addressable();
                    let name = match description.placement {
                        ModulePlacement::Declared { name, .. } => name,
                        ModulePlacement::CrateRoot | ModulePlacement::Included => String::new(),
                    };

                    graph.modules.push(Module {
                        parent: parent.map(|parent| parent.id),
                        name,
                        file: description.file,
                        inline_modules: description.inline_modules,
                        addressable,
                        crate_root,
                    });

                    let parent = ModuleParent { id, crate_root };
                    pending.extend(children.into_iter().rev().map(|child| (Some(parent), child)));
                }
                ModuleResolution::UnresolvedFiles(files) => graph.unresolved_files.extend(files),
                ModuleResolution::UnexpandedModules => {
                    if let Some(parent) = parent {
                        graph.unexpanded_roots.insert(parent.crate_root);
                    }
                }
                ModuleResolution::Missing => {}
            }
        }

        Ok(graph)
    }

    /// Resolve only the relative prefix. Item lookup and edit policy belong to the caller.
    pub(super) fn anchor(
        &self,
        file: &Path,
        inline_modules: &[String],
        parents: usize,
    ) -> Result<String, ResolutionError> {
        if self.unresolved_files.contains(file) {
            return Err(ResolutionError::UnsupportedContext);
        }

        let mut replacement = None;
        for module in self
            .modules
            .iter()
            .filter(|module| module.file == file && module.inline_modules == inline_modules)
        {
            if self.unexpanded_roots.contains(&module.crate_root) {
                return Err(ResolutionError::UnexpandedModules);
            }
            if !module.addressable {
                return Err(ResolutionError::UnsupportedContext);
            }

            let mut ancestor = module;
            for _ in 0..parents {
                ancestor = ancestor
                    .parent
                    .and_then(|parent| self.modules.get(parent))
                    .ok_or(ResolutionError::AboveCrateRoot)?;
            }

            let path = self.absolute_path(ancestor);
            if replacement.as_ref().is_some_and(|previous| previous != &path) {
                return Err(ResolutionError::ConflictingContexts);
            }
            replacement = Some(path);
        }

        replacement.ok_or(ResolutionError::UnknownModule)
    }

    pub(super) fn matches_structure(&self, file: &Path, parsed: &File) -> bool {
        self.structures
            .get(file)
            .is_none_or(|previous| *previous == ModuleStructure::collect(&parsed.items))
    }

    fn absolute_path(&self, module: &Module) -> String {
        let mut names = Vec::new();
        let mut current = module;

        while let Some(parent) = current.parent.and_then(|parent| self.modules.get(parent)) {
            names.push(current.name.as_str());
            current = parent;
        }

        names.push("crate");
        names.reverse();

        names.join("::")
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) enum ResolutionError {
    UnknownModule,
    ConflictingContexts,
    UnsupportedContext,
    AboveCrateRoot,
    UnexpandedModules,
}

struct Module {
    parent: Option<usize>,
    name: String,
    file: PathBuf,
    inline_modules: Vec<String>,
    addressable: bool,
    crate_root: usize,
}

#[derive(Clone, Copy)]
struct ModuleParent {
    id: usize,
    crate_root: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_graph_when_sources_have_aliases_reuses_canonical_snapshots_and_preserves_occurrences() {
        let root = PathBuf::from("/src/lib.rs");
        let shared = PathBuf::from("/src/shared.rs");
        let mut reads = Vec::new();

        let graph = ModuleGraph::discover(std::slice::from_ref(&root), |path| {
            reads.push(path.to_owned());

            let source = if path == root {
                SourceFile {
                    path: root.clone(),
                    text: r#"
                        #[path = "alias.rs"]
                        mod first;
                        #[path = "shared.rs"]
                        mod second;
                        #[path = "alias.rs"]
                        mod third;
                    "#
                    .to_owned(),
                }
            } else {
                SourceFile {
                    path: shared.clone(),
                    text: String::new(),
                }
            };

            Ok(source)
        })
        .unwrap();

        assert_eq!(reads, vec![root, PathBuf::from("/src/alias.rs")]);
        assert_eq!(graph.anchor(&shared, &[], 1), Ok("crate".to_owned()));
        assert_eq!(graph.anchor(&shared, &[], 0), Err(ResolutionError::ConflictingContexts));
    }

    #[test]
    fn test_module_graph_when_a_cycle_uses_a_file_alias_rejects_its_canonical_context() {
        let root = PathBuf::from("/src/lib.rs");

        let graph = ModuleGraph::discover(std::slice::from_ref(&root), |_| {
            Ok(SourceFile {
                path: root.clone(),
                text: r#"
                #[path = "alias.rs"]
                mod recursive;
            "#
                .to_owned(),
            })
        })
        .unwrap();

        assert_eq!(graph.anchor(&root, &[], 0), Err(ResolutionError::UnsupportedContext));
    }

    #[rstest::rstest]
    #[case::missing(None, ResolutionError::UnknownModule)]
    #[case::invalid(
        Some(
            r"
        fn unfinished(
    "
        ),
        ResolutionError::UnsupportedContext
    )]
    fn test_module_graph_when_a_root_is_unavailable_returns_its_resolution_status(
        #[case] source: Option<&str>,
        #[case] expected: ResolutionError,
    ) {
        let root = PathBuf::from("/src/lib.rs");

        let graph = ModuleGraph::discover(std::slice::from_ref(&root), |_| {
            source
                .map(|source| SourceFile {
                    path: root.clone(),
                    text: source.to_owned(),
                })
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        })
        .unwrap();

        assert_eq!(graph.anchor(&root, &[], 0), Err(expected));
    }

    #[rstest::rstest]
    #[case::permission(io::ErrorKind::PermissionDenied)]
    #[case::interrupted(io::ErrorKind::Interrupted)]
    fn test_module_graph_when_reading_fails_propagates_the_io_error(#[case] kind: io::ErrorKind) {
        let outcome = ModuleGraph::discover(&[PathBuf::from("/src/lib.rs")], |_| Err(io::Error::from(kind)));

        assert_eq!(outcome.map(|_| ()).map_err(|error| error.kind()), Err(kind));
    }

    #[rstest::rstest]
    #[case::named_file("/src/outer.rs", "/src/outer/inner.rs", "mod outer;", "mod inner;")]
    #[case::mod_rs("/src/outer/mod.rs", "/src/outer/inner.rs", "mod outer;", "mod inner;")]
    #[case::attributed_file(
        "/src/custom.rs",
        "/src/inner.rs",
        "#[path = \"custom.rs\"] mod outer;",
        "mod inner;"
    )]
    #[case::attributed_child("/src/outer.rs", "/src/inner.rs", "mod outer;", "#[path = \"inner.rs\"] mod inner;")]
    fn test_module_graph_when_discovering_external_modules_follows_rust_file_layout(
        #[case] outer: &str,
        #[case] inner: &str,
        #[case] root_source: &str,
        #[case] outer_source: &str,
    ) {
        let graph = discover(
            &[("/src/lib.rs", root_source), (outer, outer_source), (inner, "")],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new(inner), &[], 0),
            Ok("crate::outer::inner".to_owned())
        );

        assert_eq!(graph.anchor(Path::new(inner), &[], 1), Ok("crate::outer".to_owned()));
    }

    #[test]
    fn test_module_graph_when_inline_modules_override_directories_follows_nested_paths() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r"
                    mod outer;
                ",
                ),
                (
                    "/src/outer.rs",
                    r#"
                    mod inline {
                        #[path = "custom.rs"]
                        mod child;
                    }
                    #[path = "alternate"]
                    mod other {
                        mod child;
                    }
                "#,
                ),
                ("/src/outer/inline/custom.rs", ""),
                ("/src/alternate/child.rs", ""),
            ],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/outer/inline/custom.rs"), &[], 0),
            Ok("crate::outer::inline::child".to_owned())
        );

        assert_eq!(
            graph.anchor(Path::new("/src/alternate/child.rs"), &[], 0),
            Ok("crate::outer::other::child".to_owned())
        );
    }

    #[test]
    fn test_module_graph_when_files_are_shared_requires_replacements_to_agree() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r#"
                    #[path = "shared.rs"]
                    mod first;
                    #[path = "shared.rs"]
                    mod second;
                "#,
                ),
                (
                    "/src/shared.rs",
                    r"
                    mod nested {}
                ",
                ),
            ],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &[], 1),
            Ok("crate".to_owned())
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &[], 0),
            Err(ResolutionError::ConflictingContexts)
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &["nested".to_owned()], 1),
            Err(ResolutionError::ConflictingContexts)
        );
    }

    #[test]
    fn test_module_graph_when_attributes_are_conditional_discovers_all_possible_files() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r#"
                    #[cfg_attr(feature = "alternate", path = "alternate.rs")]
                    mod child;
                "#,
                ),
                ("/src/child.rs", ""),
                ("/src/alternate.rs", ""),
            ],
            &["/src/lib.rs"],
        );

        for file in ["/src/child.rs", "/src/alternate.rs"] {
            assert_eq!(graph.anchor(Path::new(file), &[], 0), Ok("crate::child".to_owned()));
        }
    }

    #[test]
    fn test_module_graph_when_a_file_has_multiple_crate_roots_checks_every_root() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r#"
                    #[path = "shared.rs"]
                    mod shared;
                "#,
                ),
                ("/src/shared.rs", ""),
            ],
            &["/src/lib.rs", "/src/shared.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &[], 0),
            Err(ResolutionError::ConflictingContexts)
        );
    }

    #[test]
    fn test_module_graph_when_a_module_is_local_rejects_crate_root_anchoring() {
        let graph = discover(
            &[(
                "/src/lib.rs",
                r"
            fn run() {
                mod local {}
            }
        ",
            )],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &["local".to_owned()], 0),
            Err(ResolutionError::UnsupportedContext)
        );

        assert_eq!(graph.anchor(Path::new("/src/lib.rs"), &[], 0), Ok("crate".to_owned()));
        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &[], 1),
            Err(ResolutionError::AboveCrateRoot)
        );

        assert_eq!(
            graph.anchor(Path::new("/src/unknown.rs"), &[], 0),
            Err(ResolutionError::UnknownModule)
        );
    }

    #[test]
    fn test_module_graph_when_modules_form_a_cycle_stops_and_rejects_affected_files() {
        let graph = discover(
            &[(
                "/src/lib.rs",
                r#"
            #[path = "lib.rs"]
            mod recursive;
        "#,
            )],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &[], 0),
            Err(ResolutionError::UnsupportedContext)
        );
    }

    #[test]
    fn test_module_graph_when_both_default_files_exist_rejects_ambiguous_sources() {
        let graph = discover(
            &[
                ("/src/lib.rs", "mod child;"),
                ("/src/child.rs", ""),
                ("/src/child/mod.rs", ""),
            ],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/child.rs"), &[], 0),
            Err(ResolutionError::UnsupportedContext)
        );

        assert_eq!(
            graph.anchor(Path::new("/src/child/mod.rs"), &[], 0),
            Err(ResolutionError::UnsupportedContext)
        );
    }

    #[test]
    fn test_module_graph_when_source_is_also_included_rejects_anchoring() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r#"
                mod shared;
                mod other {
                    include!("shared.rs");
                }
            "#,
                ),
                (
                    "/src/shared.rs",
                    r"
                mod nested {}
            ",
                ),
            ],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &[], 1),
            Err(ResolutionError::UnsupportedContext)
        );

        assert_eq!(
            graph.anchor(Path::new("/src/shared.rs"), &["nested".to_owned()], 0),
            Err(ResolutionError::UnsupportedContext)
        );
    }

    #[test]
    fn test_module_graph_when_included_tokens_are_an_expression_rejects_hidden_contexts() {
        let graph = discover(
            &[
                (
                    "/src/lib.rs",
                    r#"
                fn run() -> usize {
                    include!("expression.rs")
                }
                mod ordinary {}
            "#,
                ),
                (
                    "/src/expression.rs",
                    r#"
                {
                    include!("hidden.rs");
                    0
                }
            "#,
                ),
            ],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &["ordinary".to_owned()], 0),
            Err(ResolutionError::UnexpandedModules)
        );
    }

    #[test]
    fn test_module_graph_when_a_module_has_an_unknown_attribute_uses_its_declared_context() {
        let graph = discover(
            &[(
                "/src/lib.rs",
                r#"
            #[cfg_attr(feature = "generated", generate)]
            mod altered {
                mod nested {}
            }
            mod ordinary {}
        "#,
            )],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(
                Path::new("/src/lib.rs"),
                &["altered".to_owned(), "nested".to_owned()],
                0
            ),
            Ok("crate::altered::nested".to_owned())
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &["ordinary".to_owned()], 0),
            Ok("crate::ordinary".to_owned())
        );
    }

    #[test]
    fn test_module_graph_when_an_item_macro_is_present_uses_explicit_modules() {
        let graph = discover(
            &[(
                "/src/lib.rs",
                r"
                generate_modules!();
                mod ordinary {}
            ",
            )],
            &["/src/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &["ordinary".to_owned()], 0),
            Ok("crate::ordinary".to_owned())
        );
    }

    #[test]
    fn test_module_graph_when_an_include_path_is_computed_blocks_the_affected_crate() {
        let source = r#"
            include!(concat!(env!("OUT_DIR"), "/generated.rs"));
            mod ordinary {}
        "#;

        let graph = discover(
            &[
                ("/src/lib.rs", source),
                (
                    "/other/lib.rs",
                    r"
            mod ordinary {}
        ",
                ),
            ],
            &["/src/lib.rs", "/other/lib.rs"],
        );

        assert_eq!(
            graph.anchor(Path::new("/src/lib.rs"), &["ordinary".to_owned()], 0),
            Err(ResolutionError::UnexpandedModules)
        );

        assert_eq!(
            graph.anchor(Path::new("/other/lib.rs"), &["ordinary".to_owned()], 0),
            Ok("crate::ordinary".to_owned())
        );
    }

    #[test]
    fn test_module_graph_when_source_changes_only_invalidates_module_structure_changes() {
        let graph = discover(
            &[(
                "/src/lib.rs",
                r"
            mod first {}
            mod second {}
            fn run() {}
        ",
            )],
            &["/src/lib.rs"],
        );

        let reordered = syn::parse_file(
            r"
            fn run() { let _ = 1; }
            mod second {}
            mod first {}
        ",
        )
        .unwrap();

        assert!(graph.matches_structure(Path::new("/src/lib.rs"), &reordered));

        let renamed = syn::parse_file(
            r"
            mod renamed {}
            mod second {}
        ",
        )
        .unwrap();

        assert!(!graph.matches_structure(Path::new("/src/lib.rs"), &renamed));
    }

    fn discover(files: &[(&str, &str)], roots: &[&str]) -> ModuleGraph {
        let roots: Vec<_> = roots.iter().map(PathBuf::from).collect();
        ModuleGraph::discover(&roots, |path| {
            let text = files
                .iter()
                .find(|(name, _)| Path::new(name) == path)
                .map(|(_, text)| (*text).to_owned())
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            Ok(SourceFile {
                path: path.to_owned(),
                text,
            })
        })
        .unwrap()
    }
}
