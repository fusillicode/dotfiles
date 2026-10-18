//! Import requests reserve their names before any references are shortened.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write;

use syn::Block;
use syn::ConstParam;
use syn::Item;
use syn::ItemMacro;
use syn::ItemMod;
use syn::Macro;
use syn::PatIdent;
use syn::Stmt;
use syn::TypeParam;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::module_idx::ModuleScope;
use super::module_idx::ScopeInfo;
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;
use crate::cmds::rsl::source::item_ranges_with_attached_comments;

pub(in crate::cmds::rsl::rules) enum ImportError {
    UnknownImports,
    UnknownBindings,
    NameConflict { name: String },
    ConflictingImport { name: String, existing: String },
}

impl From<ImportError> for FixBlockReason {
    fn from(error: ImportError) -> Self {
        match error {
            ImportError::UnknownImports => Self {
                code: "unknown-imports",
                message: "A wildcard, attributed, or absolute import prevents checking all available names.".to_owned(),
                help: "Use explicit imports so the names available in this module can be checked.",
            },
            ImportError::UnknownBindings => Self {
                code: "unknown-bindings",
                message: "A macro or unresolved block import may introduce conflicting names.".to_owned(),
                help: "Review the affected names and make this change manually if appropriate.",
            },
            ImportError::NameConflict { name } => Self {
                code: "name-conflict",
                message: format!("The name `{name}` may refer to another item or local binding."),
                help: "Keep the existing spelling, or rename the conflicting item.",
            },
            ImportError::ConflictingImport { name, existing } => Self {
                code: "name-conflict",
                message: format!("The name `{name}` is already imported from `{existing}`."),
                help: "Keep the existing spelling, or rename the conflicting item.",
            },
        }
    }
}

pub(in crate::cmds::rsl::rules) struct ImportEdits {
    bindings: HashMap<String, String>,
    reserved: LocalNames,
    unknown: bool,
    additions: BTreeSet<String>,
}

impl ImportEdits {
    // Seed reservations from the scope before requests can introduce any new names.
    pub(in crate::cmds::rsl::rules) fn new(scope: &ModuleScope<'_>, info: &ScopeInfo) -> Self {
        let mut reserved = local_names(scope.items);
        reserved.names.extend(info.definitions.iter().cloned());

        Self {
            bindings: info
                .imports
                .iter()
                .map(|binding| (binding.name.clone(), binding.source_path.join("::")))
                .collect(),
            reserved,
            unknown: info.unknown_imports,
            additions: BTreeSet::new(),
        }
    }

    pub(in crate::cmds::rsl::rules) fn request(&mut self, path: &str) -> Result<(), ImportError> {
        let name = path.rsplit("::").next().unwrap_or(path);

        if self.unknown {
            return Err(ImportError::UnknownImports);
        }
        if self.reserved.has_unknown_bindings() {
            return Err(ImportError::UnknownBindings);
        }
        if self.reserved.contains(name) {
            return Err(ImportError::NameConflict { name: name.to_owned() });
        }

        if let Some(existing) = self.bindings.get(name) {
            if existing == path {
                return Ok(());
            }

            return Err(ImportError::ConflictingImport {
                name: name.to_owned(),
                existing: existing.clone(),
            });
        }

        self.bindings.insert(name.to_owned(), path.to_owned());
        self.additions.insert(path.to_owned());

        Ok(())
    }

    pub(in crate::cmds::rsl::rules) fn apply(
        self,
        ctx: &FileContext<'_>,
        scope: &ModuleScope<'_>,
        editor: &mut SourceEditor<'_>,
    ) -> rootcause::Result<()> {
        if self.additions.is_empty() {
            return Ok(());
        }

        let spans: Vec<_> = scope.items.iter().map(Spanned::span).collect();
        let ranges = item_ranges_with_attached_comments(ctx.source, &spans, scope.body_start);
        let offset = ranges
            .first()
            .map_or_else(|| editor.offset(scope.body_start), |range| range.byte_range.start);

        let newline = if ctx.source.contains("\r\n") { "\r\n" } else { "\n" };
        let prefix = ctx.source.get(..offset).unwrap_or_default();
        let suffix = ctx.source.get(offset..).unwrap_or_default();
        let indentation = suffix
            .split(|character| !matches!(character, ' ' | '\t'))
            .next()
            .unwrap_or_default();
        let mut insertion = if prefix.is_empty() || prefix.ends_with('\n') {
            String::new()
        } else {
            newline.to_owned()
        };

        for path in self.additions {
            write!(insertion, "{indentation}use {path};{newline}")?;
        }

        editor.replace(offset..offset, insertion)
    }
}

pub(in crate::cmds::rsl::rules) fn local_names(items: &[Item]) -> LocalNames {
    let mut visitor = LocalNames {
        names: HashSet::new(),
        unknown: false,
    };

    for item in items {
        visitor.visit_item(item);
    }

    visitor
}

pub(in crate::cmds::rsl::rules) struct LocalNames {
    names: HashSet<String>,
    unknown: bool,
}

impl LocalNames {
    pub(in crate::cmds::rsl::rules) const fn has_unknown_bindings(&self) -> bool {
        self.unknown
    }

    pub(in crate::cmds::rsl::rules) fn contains(&self, name: &str) -> bool {
        self.unknown || self.names.contains(name)
    }
}

impl<'ast> Visit<'ast> for LocalNames {
    fn visit_pat_ident(&mut self, pattern: &'ast PatIdent) {
        self.names.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }

    fn visit_type_param(&mut self, parameter: &'ast TypeParam) {
        self.names.insert(parameter.ident.to_string());
        syn::visit::visit_type_param(self, parameter);
    }

    fn visit_const_param(&mut self, parameter: &'ast ConstParam) {
        self.names.insert(parameter.ident.to_string());
        syn::visit::visit_const_param(self, parameter);
    }

    fn visit_block(&mut self, block: &'ast Block) {
        for statement in &block.stmts {
            let Stmt::Item(item) = statement else {
                continue;
            };

            if let Some(name) = super::module_idx::item_name(item) {
                self.names.insert(name);
            }

            if let Item::Use(item_use) = item {
                let bindings = super::import_resolution::use_bindings(&item_use.tree, &[]);
                self.unknown |= bindings.unknown;
                self.names
                    .extend(bindings.bindings.into_iter().map(|binding| binding.name));
            }
        }

        syn::visit::visit_block(self, block);
    }

    fn visit_item_mod(&mut self, _module: &'ast ItemMod) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {
        // An expansion can introduce imports or local bindings that the syntax tree cannot resolve.
        self.unknown = true;
    }

    fn visit_item_macro(&mut self, item: &'ast ItemMacro) {
        if item.ident.is_none() {
            self.unknown = true;
        }
    }
}
