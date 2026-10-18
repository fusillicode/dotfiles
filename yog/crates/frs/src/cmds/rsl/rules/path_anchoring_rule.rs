//! Anchor references starting with `self::` or `super::` at the crate's root.
//!
//! Prefer a path that names the module from the crate's root, so its meaning does not depend on nesting.
//! For example, prefer `crate::settings::Config` over `super::Config` when the parent module is `settings`.
//!
//! References are changed only when their containing module is known and all uses of that source file
//! agree on the replacement. A private `use super::*;` inside a test module is allowed.

use proc_macro2::Span;
use syn::Attribute;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::Path;
use syn::UseTree;
use syn::Visibility;
use syn::visit;
use syn::visit::Visit;

use super::Rule;
use super::common;
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::ast;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::mod_resolution::ResolutionError;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct PathAnchoringRule;

impl Rule for PathAnchoringRule {
    fn id(&self) -> &'static str {
        "path-anchoring"
    }

    fn needs_module_context(&self) -> bool {
        true
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let mut visitor = RelativeModuleReferences {
            test_module: false,
            prefixes: Vec::new(),
            inline_modules: Vec::new(),
        };

        visitor.visit_file(ctx.file);

        for prefix in visitor.prefixes {
            let text = plan.edits.source_range(plan.edits.span_range(prefix.span));
            let change = Change {
                span: prefix.span,
                path: text,
            };

            if let Err(reason) = common::ensure_no_comments(&plan.edits, prefix.span) {
                plan.record_blocked(&change, reason);
                continue;
            }

            match ctx.modules.anchor(ctx.path, &prefix.inline_modules, prefix.parents) {
                Ok(replacement) => {
                    plan.edits.replace_span(prefix.span, replacement)?;
                    plan.record_fixable(&change);
                }
                Err(reason) => plan.record_blocked(&change, module_context_reason(&reason)),
            }
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
        if self.path == "self" {
            format!("replace current-module reference `{}`", self.path)
        } else {
            format!("replace parent-module reference `{}`", self.path)
        }
    }
}

fn module_context_reason(reason: &ResolutionError) -> FixBlockReason {
    match reason {
        ResolutionError::UnknownModule => FixBlockReason {
            code: "parent-module-unknown",
            message: "The required crate or parent-module context is outside this file.".to_owned(),
            help: "Use the correct crate:: path after checking the surrounding modules.",
        },
        ResolutionError::ConflictingContexts => FixBlockReason {
            code: "conflicting-module-contexts",
            message: "This source belongs to modules requiring different crate-root paths.".to_owned(),
            help: "Keep the relative path, or stop sharing the source between different modules.",
        },
        ResolutionError::UnsupportedContext => FixBlockReason {
            code: "unsupported-module-context",
            message: "This module has no unambiguous path from the crate root.".to_owned(),
            help: "Keep the relative path in local, circular, or ambiguous modules.",
        },
        ResolutionError::AboveCrateRoot => FixBlockReason {
            code: "above-crate-root",
            message: "The reference goes beyond the crate's root module.".to_owned(),
            help: "Correct the number of super:: segments.",
        },
        ResolutionError::UnexpandedModules => FixBlockReason {
            code: "unexpanded-modules",
            message: "Macros may introduce additional module locations in this crate.".to_owned(),
            help: "Keep relative paths when module locations depend on macro expansion.",
        },
    }
}

struct RelativePrefix {
    span: Span,
    parents: usize,
    inline_modules: Vec<String>,
}

struct RelativeModuleReferences {
    test_module: bool,
    prefixes: Vec<RelativePrefix>,
    inline_modules: Vec<String>,
}

impl RelativeModuleReferences {
    fn prefix(&mut self, spans: &[Span], parents: usize) {
        let (Some(first), Some(last)) = (spans.first(), spans.last()) else {
            return;
        };

        let Some(span) = first.join(*last) else {
            return;
        };

        self.prefixes.push(RelativePrefix {
            span,
            parents,
            inline_modules: self.inline_modules.clone(),
        });
    }

    fn use_tree(&mut self, tree: &UseTree) {
        let mut pending = vec![tree];

        while let Some(tree) = pending.pop() {
            let mut current = tree;
            let mut spans = Vec::new();
            let mut parents = 0_usize;

            if let UseTree::Path(path) = current
                && path.ident == "self"
            {
                self.prefix(&[path.ident.span()], 0);
                continue;
            }

            while let UseTree::Path(path) = current
                && path.ident == "super"
            {
                spans.push(path.ident.span());
                parents = parents.saturating_add(1);
                current = &path.tree;
            }

            match current {
                UseTree::Name(name) if name.ident == "super" => {
                    spans.push(name.ident.span());
                    parents = parents.saturating_add(1);
                }
                UseTree::Rename(rename) if rename.ident == "super" => {
                    spans.push(rename.ident.span());
                    parents = parents.saturating_add(1);
                }
                UseTree::Name(_) | UseTree::Rename(_) | UseTree::Path(_) | UseTree::Group(_) | UseTree::Glob(_) => {}
            }

            if !spans.is_empty() {
                self.prefix(&spans, parents);
                continue;
            }

            if let UseTree::Group(group) = tree {
                pending.extend(group.items.iter().rev());
            }
        }
    }
}

impl<'ast> Visit<'ast> for RelativeModuleReferences {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        self.visit_visibility(&module.vis);

        let Some((_, items)) = &module.content else {
            return;
        };

        let previous = self.test_module;
        self.test_module |= ast::is_test_module_declaration(module);
        self.inline_modules.push(module.ident.to_string());

        for item in items {
            self.visit_item(item);
        }

        self.inline_modules.pop();
        self.test_module = previous;
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.visit_visibility(&item.vis);
        let allowed = self.test_module
            && matches!(item.vis, Visibility::Inherited)
            && matches!(&item.tree, UseTree::Path(path) if path.ident == "super" && matches!(path.tree.as_ref(), UseTree::Glob(_)));

        if !allowed && item.leading_colon.is_none() {
            self.use_tree(&item.tree);
        }
    }

    fn visit_path(&mut self, path: &'ast Path) {
        if path.leading_colon.is_none()
            && path.segments.len() > 1
            && let Some(segment) = path.segments.first().filter(|segment| segment.ident == "self")
        {
            self.prefix(&[segment.ident.span()], 0);
        } else if path.leading_colon.is_none() {
            let spans: Vec<_> = path
                .segments
                .iter()
                .take_while(|segment| segment.ident == "super")
                .map(|segment| segment.ident.span())
                .collect();
            self.prefix(&spans, spans.len());
        }

        visit::visit_path(self, path);
    }

    fn visit_visibility(&mut self, visibility: &'ast Visibility) {
        if let Visibility::Restricted(restricted) = visibility
            && restricted.in_token.is_some()
        {
            self.visit_path(&restricted.path);
        }
    }

    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {}
}
