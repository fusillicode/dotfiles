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
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;
use crate::cmds::rsl::source::item_ranges;

pub(in crate::cmds::rsl::rules) struct ImportEdits {
    bindings: HashMap<String, String>,
    reserved: LocalNames,
    unknown: bool,
    additions: BTreeSet<String>,
}

impl ImportEdits {
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

    pub(in crate::cmds::rsl::rules) fn request(&mut self, path: &str, editor: &mut SourceEditor<'_>) -> bool {
        let Some(name) = path.rsplit("::").next() else {
            return false;
        };

        if self.unknown || self.reserved.contains(name) {
            editor.skip(format!("cannot import {path}: name may be shadowed"));
            return false;
        }

        if let Some(existing) = self.bindings.get(name) {
            if existing == path {
                return true;
            }

            editor.skip(format!("cannot import {path}: {name} is already bound"));
            return false;
        }

        self.bindings.insert(name.to_owned(), path.to_owned());
        self.additions.insert(path.to_owned());

        true
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
        let ranges = item_ranges::item_ranges_with_attached_comments(ctx.source, &spans, scope.body_start);
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
