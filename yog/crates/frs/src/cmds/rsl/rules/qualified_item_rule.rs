//! Import items such as types so their names are easier to read where they are used.
//!
//! For example, add `use std::path::PathBuf;` and write `PathBuf` instead of `std::path::PathBuf`.
//! The same applies to uses such as `PathBuf::new()`.
//!
//! Keep module names on common names such as `Result`, `Error`, and `Option` to make their meaning clear.
//! Function calls are handled by separate rules. Leave names unchanged when shortening them could cause
//! a conflict or when this file does not provide enough information to check the change.

use proc_macro2::Span;
use syn::Attribute;
use syn::ItemMod;
use syn::ItemUse;
use syn::Macro;
use syn::Path;
use syn::visit::Visit;

use super::Rule;
use super::common::ImportEdits;
use super::common::ImportError;
use super::common::ensure_no_comments;
use super::common::has_name_clash_parts;
use super::common::is_import_style_path;
use super::common::local_names;
use super::common::module_idx;
use super::common::module_idx::ModuleIdx;
use super::common::module_idx::ModuleScope;
use super::common::path_resolution::normalize_path;
use crate::cmds::rsl::analysis::FixBlockReason;
use crate::cmds::rsl::analysis::RuleChange;
use crate::cmds::rsl::analysis::RulePlan;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

const KEEP_QUALIFIED_NAMES: &[&str] = &[
    "AsMut",
    "AsRef",
    "AsyncFn",
    "AsyncFnMut",
    "AsyncFnOnce",
    "Box",
    "Clone",
    "Copy",
    "Default",
    "DoubleEndedIterator",
    "Drop",
    "Eq",
    "Err",
    "Error",
    "ExactSizeIterator",
    "Extend",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "FromIterator",
    "Future",
    "Into",
    "IntoFuture",
    "IntoIterator",
    "Iterator",
    "None",
    "Ok",
    "Option",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "Result",
    "Send",
    "Sized",
    "Some",
    "String",
    "Sync",
    "ToOwned",
    "ToString",
    "TryFrom",
    "TryInto",
    "Unpin",
    "Vec",
];

pub(super) struct QualifiedItemRule;

impl Rule for QualifiedItemRule {
    fn id(&self) -> &'static str {
        "qualified-item"
    }

    fn analyze<'source>(&self, ctx: &FileContext<'source>) -> rootcause::Result<RulePlan<'source>> {
        let mut plan = RulePlan {
            edits: SourceEditor::new(ctx.source),
            findings: Vec::new(),
        };
        let idx = module_idx(ctx.file);

        for scope in &idx.scopes {
            analyze_scope(ctx, &idx, scope, &mut plan)?;
        }

        Ok(plan)
    }
}

struct Change<'path> {
    span: Span,
    path: &'path str,
    name: &'path str,
}

impl Change<'_> {
    fn add_to_plan(&self, plan: &mut RulePlan<'_>) -> rootcause::Result<()> {
        plan.edits.replace_span(self.span, self.name)?;
        plan.record_fixable(self);
        Ok(())
    }
}

impl RuleChange for Change<'_> {
    fn span(&self) -> Span {
        self.span
    }

    fn description(&self) -> String {
        format!("replace `{}` with `{}`", self.path, self.name)
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

fn analyze_scope(
    ctx: &FileContext<'_>,
    idx: &ModuleIdx<'_>,
    scope: &ModuleScope<'_>,
    plan: &mut RulePlan<'_>,
) -> rootcause::Result<()> {
    let Some(info) = idx.info.get(&scope.path) else {
        return Ok(());
    };

    let mut visitor = QualifiedPaths { paths: Vec::new() };

    for item in scope.items {
        visitor.visit_item(item);
    }

    let mut imports = ImportEdits::new(scope, info);
    let local = local_names(scope.items);

    for (span, parts) in visitor.paths {
        let path = parts.join("::");
        let Some(name) = parts.last() else {
            continue;
        };

        if KEEP_QUALIFIED_NAMES.contains(&name.as_str()) {
            continue;
        }

        let change = Change {
            span,
            path: &path,
            name,
        };

        if let Err(reason) = ensure_no_comments(&plan.edits, span) {
            plan.record_blocked(&change, reason);
            continue;
        }

        if info.unknown_imports {
            plan.record_blocked(&change, ImportError::UnknownImports);
            continue;
        }

        if has_name_clash_parts(idx, &scope.path, &parts) {
            plan.record_blocked(&change, ImportError::NameConflict { name: name.clone() });
            continue;
        }

        if matches!(parts.first().map(String::as_str), Some("crate" | "super")) {
            plan.record_blocked(&change, BlockReason::MissingModuleContext);
            continue;
        }

        let same_module = normalize_path(&scope.path, &parts)
            == scope
                .path
                .iter()
                .cloned()
                .chain(std::iter::once(name.clone()))
                .collect::<Vec<_>>();

        if same_module && local.contains(name) {
            let reason = if local.has_unknown_bindings() {
                ImportError::UnknownBindings
            } else {
                ImportError::NameConflict { name: name.clone() }
            };
            plan.record_blocked(&change, reason);
            continue;
        }

        if !(same_module && info.definitions.contains(name))
            && let Err(reason) = imports.request(&path)
        {
            plan.record_blocked(&change, reason);
            continue;
        }

        change.add_to_plan(plan)?;
    }

    imports.apply(ctx, scope, &mut plan.edits)
}

struct QualifiedPaths {
    paths: Vec<(Span, Vec<String>)>,
}

impl QualifiedPaths {
    fn collect(&mut self, path: &Path) {
        if path.leading_colon.is_some() {
            return;
        }

        let segments: Vec<_> = path.segments.iter().collect();
        let Some(type_position) = segments
            .iter()
            .position(|segment| segment.ident.to_string().chars().next().is_some_and(char::is_uppercase))
        else {
            return;
        };

        let end = type_position.saturating_add(1);
        let prefix = segments.get(..end).unwrap_or_default();

        if prefix.len() < 2 {
            return;
        }

        let parts: Vec<_> = prefix.iter().map(|segment| segment.ident.to_string()).collect();

        if !is_import_style_path(&parts) {
            return;
        }

        if let (Some(first), Some(last)) = (prefix.first(), prefix.last())
            && let Some(span) = first.ident.span().join(last.ident.span())
        {
            self.paths.push((span, parts));
        }
    }
}

impl<'ast> Visit<'ast> for QualifiedPaths {
    fn visit_path(&mut self, path: &'ast Path) {
        self.collect(path);
        syn::visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _module: &'ast ItemMod) {}

    fn visit_item_use(&mut self, _item: &'ast ItemUse) {}

    fn visit_attribute(&mut self, _attribute: &'ast Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast Macro) {}
}
