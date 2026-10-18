//! Flag references that start with `super::`, which point to a parent module.
//!
//! Prefer a path that names the module from the crate's root, so its meaning does not depend on nesting.
//! For example, prefer `crate::settings::Config` over `super::Config` when the parent module is `settings`.
//!
//! This rule reports these references without changing them because the parent module cannot be identified
//! from this file alone. A private `use super::*;` inside a test module is allowed.

use proc_macro2::Span;
use syn::Attribute;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::Path;
use syn::UseTree;
use syn::Visibility;
use syn::visit::Visit;

use super::Rule;
use super::common::ensure_no_comments;
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::ast::is_test_module_declaration;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct PathAnchoringRule;

impl Rule for PathAnchoringRule {
    fn id(&self) -> &'static str {
        "path-anchoring"
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let mut visitor = ParentModuleReferences {
            test_module: false,
            prefixes: Vec::new(),
        };
        visitor.visit_file(ctx.file);

        for prefix in visitor.prefixes {
            let text = plan.edits.source_range(plan.edits.span_range(prefix));
            let change = Change {
                span: prefix,
                path: text,
            };
            let reason = match ensure_no_comments(&plan.edits, prefix) {
                Ok(()) => BlockReason::MissingModuleContext.into(),
                Err(reason) => reason,
            };
            plan.record_blocked(&change, reason);
        }

        Ok(plan)
    }
}

struct Change<'path> {
    span: Span,
    path: &'path str,
}

impl RuleChange for Change<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace parent-module reference `{}`", self.path)
    }
}

enum BlockReason {
    MissingModuleContext,
}

impl From<BlockReason> for FixBlockReason {
    fn from(reason: BlockReason) -> Self {
        match reason {
            BlockReason::MissingModuleContext => Self {
                code: "parent-module-unknown",
                message: "The required crate or parent-module context is outside this file.".to_owned(),
                help: "Use the correct crate:: path after checking the surrounding modules.",
            },
        }
    }
}

struct ParentModuleReferences {
    test_module: bool,
    prefixes: Vec<Span>,
}

impl ParentModuleReferences {
    fn prefix(&mut self, spans: &[Span]) {
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            return;
        };

        let Some(span) = first.join(*last) else {
            return;
        };

        self.prefixes.push(span);
    }

    fn use_tree(&mut self, tree: &UseTree) {
        let mut pending = vec![tree];

        while let Some(tree) = pending.pop() {
            let mut current = tree;
            let mut spans = Vec::new();

            while let UseTree::Path(path) = current
                && path.ident == "super"
            {
                spans.push(path.ident.span());
                current = &path.tree;
            }

            match current {
                UseTree::Name(name) if name.ident == "super" => spans.push(name.ident.span()),
                UseTree::Rename(rename) if rename.ident == "super" => spans.push(rename.ident.span()),
                UseTree::Name(_) | UseTree::Rename(_) | UseTree::Path(_) | UseTree::Group(_) | UseTree::Glob(_) => {}
            }

            if !spans.is_empty() {
                self.prefix(&spans);
                continue;
            }

            if let UseTree::Group(group) = tree {
                pending.extend(group.items.iter().rev());
            }
        }
    }
}

impl<'ast> Visit<'ast> for ParentModuleReferences {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        let previous = self.test_module;
        self.test_module |= is_test_module_declaration(module);

        syn::visit::visit_item_mod(self, module);

        self.test_module = previous;
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        let allowed = self.test_module
            && matches!(item.vis, Visibility::Inherited)
            && matches!(&item.tree, UseTree::Path(path) if path.ident == "super" && matches!(path.tree.as_ref(), UseTree::Glob(_)));

        if !allowed && item.leading_colon.is_none() {
            self.use_tree(&item.tree);
        }
    }

    fn visit_path(&mut self, path: &'ast Path) {
        if path.leading_colon.is_none() {
            let spans: Vec<_> = path
                .segments
                .iter()
                .take_while(|segment| segment.ident == "super")
                .map(|segment| segment.ident.span())
                .collect();
            self.prefix(&spans);
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {}
}
