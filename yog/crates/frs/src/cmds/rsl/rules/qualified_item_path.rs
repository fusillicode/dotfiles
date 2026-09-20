//! Qualified-item-path rule for `frs rsl`.

use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;
use syn::Expr;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::common::ModuleIndex;
use super::common::associated_receiver_parts;
use super::common::has_name_clash_parts;
use super::common::is_import_style_path;
use super::common::module_index;
use super::common::path_parts;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct QualifiedItemPathViolation {
    pub(super) file: PathBuf,
    pub(super) line: usize,
    pub(super) column: usize,
    pub(super) message: &'static str,
    pub(super) details: QualifiedItemPathDetails,
}

impl QualifiedItemPathViolation {
    pub(super) fn new(path: &Path, span: Span, actual_path: String, expected_import: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "import this item",
            details: QualifiedItemPathDetails {
                actual_path,
                expected_import,
            },
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct QualifiedItemPathDetails {
    pub(super) actual_path: String,
    pub(super) expected_import: String,
}

pub struct QualifiedItemPathRule;

impl TypedRule for QualifiedItemPathRule {
    type Violation = QualifiedItemPathViolation;

    fn name() -> &'static str {
        "qualified_item_path"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let index = module_index(ctx.file);
        let mut violations = Vec::new();

        for scope in &index.scopes {
            let mut visitor = QualifiedItemPathVisitor {
                index: &index,
                current_module: &scope.path,
                source_path: ctx.path,
                violations: &mut violations,
                skip_call_path: false,
            };
            for item in scope.items {
                visitor.visit_item(item);
            }
        }

        violations
    }
}

struct QualifiedItemPathVisitor<'index, 'ast, 'output> {
    index: &'index ModuleIndex<'ast>,
    current_module: &'index [String],
    source_path: &'index Path,
    violations: &'output mut Vec<QualifiedItemPathViolation>,
    skip_call_path: bool,
}

impl<'ast> Visit<'ast> for QualifiedItemPathVisitor<'_, '_, '_> {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && let Some(parts) = associated_receiver_parts(&path.path)
        {
            check_non_function_path(self, &parts, path.path.span());
        }

        let previous = self.skip_call_path;
        self.skip_call_path = matches!(expression.func.as_ref(), Expr::Path(_));
        syn::visit::visit_expr_call(self, expression);
        self.skip_call_path = previous;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.skip_call_path {
            self.skip_call_path = false;
        } else if let Some(parts) = path_parts(path) {
            check_non_function_path(self, &parts, path.span());
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _module: &'ast syn::ItemMod) {}

    fn visit_item_use(&mut self, _item_use: &'ast syn::ItemUse) {}

    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {}
}

fn check_non_function_path(visitor: &mut QualifiedItemPathVisitor<'_, '_, '_>, parts: &[String], span: Span) {
    if parts.len() <= 1
        || !is_import_style_path(parts)
        || has_name_clash_parts(visitor.index, visitor.current_module, parts)
    {
        return;
    }

    let actual_path = parts.join("::");
    visitor.violations.push(QualifiedItemPathViolation::new(
        visitor.source_path,
        span,
        actual_path.clone(),
        format!("use {actual_path};"),
    ));
}

impl Display for QualifiedItemPathViolation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        formatter.write_str(&crate::cmds::rsl::rules::format_compact_violation(
            &self.file,
            self.line,
            self.column,
            self.message,
            &format!("{} -> {}", self.details.actual_path, self.details.expected_import),
        ))
    }
}

impl TypedRuleViolation for QualifiedItemPathViolation {
    type Rule = QualifiedItemPathRule;
}
