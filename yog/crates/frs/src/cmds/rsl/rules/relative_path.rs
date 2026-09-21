//! Relative-path rule for `frs rsl`.

use std::path::Path;

use proc_macro2::Span;
use syn::Expr;
use syn::UseTree;
use syn::spanned::Spanned;
use syn::visit::Visit;

use super::common::Location;
use crate::cmds::rsl::ast::is_test_module_declaration;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::rules::TypedRule;
use crate::cmds::rsl::rules::TypedRuleViolation;

pub struct RelativePathRule;

impl TypedRule for RelativePathRule {
    type Violation = RelativePathViolation;

    fn code() -> &'static str {
        "relative_path"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let mut violations = Vec::new();
        let mut visitor = RelativePathVisitor {
            source_path: ctx.path,
            violations: &mut violations,
            in_test_module: false,
        };

        for item in &ctx.file.items {
            visitor.visit_item(item);
        }

        violations
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct RelativePathViolation {
    pub location: Location,
}

impl RelativePathViolation {
    fn new(path: &Path, span: Span) -> Self {
        Self {
            location: Location::from_span(path, span),
        }
    }
}

impl TypedRuleViolation for RelativePathViolation {
    type Rule = RelativePathRule;
}

struct RelativePathVisitor<'output> {
    source_path: &'output Path,
    violations: &'output mut Vec<RelativePathViolation>,
    in_test_module: bool,
}

impl<'ast> Visit<'ast> for RelativePathVisitor<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let previous = self.in_test_module;
        self.in_test_module |= is_test_module_declaration(module);
        syn::visit::visit_item_mod(self, module);
        self.in_test_module = previous;
    }

    fn visit_item_use(&mut self, item_use: &'ast syn::ItemUse) {
        if !is_allowed_test_glob(item_use, self.in_test_module) {
            let mut spans = Vec::new();
            relative_use_spans(&item_use.tree, &mut spans);
            self.violations.extend(
                spans
                    .into_iter()
                    .map(|span| RelativePathViolation::new(self.source_path, span)),
            );
        }

        syn::visit::visit_item_use(self, item_use);
    }

    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref()
            && path_starts_with_super(&path.path)
        {
            self.violations
                .push(RelativePathViolation::new(self.source_path, path.path.span()));
        }

        syn::visit::visit_expr_call(self, expression);
    }

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {}
}

fn is_allowed_test_glob(item_use: &syn::ItemUse, in_test_module: bool) -> bool {
    in_test_module
        && matches!(item_use.vis, syn::Visibility::Inherited)
        && matches!(
            &item_use.tree,
            UseTree::Path(path)
                if path.ident == "super" && matches!(path.tree.as_ref(), UseTree::Glob(_))
        )
}

fn relative_use_spans(tree: &UseTree, spans: &mut Vec<Span>) {
    match tree {
        UseTree::Path(path) if path.ident == "super" => spans.push(tree.span()),
        UseTree::Group(group) => {
            for tree in &group.items {
                relative_use_spans(tree, spans);
            }
        }
        UseTree::Name(name) if name.ident == "super" => spans.push(tree.span()),
        UseTree::Rename(rename) if rename.ident == "super" => spans.push(tree.span()),
        UseTree::Path(_) | UseTree::Glob(_) | UseTree::Name(_) | UseTree::Rename(_) => {}
    }
}

fn path_starts_with_super(path: &syn::Path) -> bool {
    path.segments.first().is_some_and(|segment| segment.ident == "super")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::*;
    use crate::cmds::rsl::rules::TypedRule;
    use crate::cmds::rsl::rules::common::Location;

    #[test]
    fn test_relative_path_check_when_import_is_outside_tests_reports_violation() {
        let syntax = syn::parse_file(
            r"
            mod parent {
                fn helper() {}
                mod child {
                    use super::helper;
                }
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![RelativePathViolation {
                location: Location::new(PathBuf::from("test.rs"), 5, 25),
            }])
        );
    }

    #[test]
    fn test_relative_path_check_when_test_module_uses_super_glob_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::*;
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(result, is_empty());
    }

    #[test]
    fn test_relative_path_check_when_test_module_uses_explicit_super_import_reports_violation() {
        let syntax = syn::parse_file(
            r"
            #[cfg(test)]
            mod tests {
                use super::helper;
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![RelativePathViolation {
                location: Location::new(PathBuf::from("test.rs"), 4, 21),
            }])
        );
    }

    #[test]
    fn test_relative_path_check_when_call_uses_super_reports_violation() {
        let syntax = syn::parse_file(
            r"
            mod parent {
                fn helper() {}
                mod child {
                    fn run() {
                        super::helper();
                    }
                }
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![RelativePathViolation {
                location: Location::new(PathBuf::from("test.rs"), 6, 25),
            }])
        );
    }

    #[test]
    fn test_relative_path_check_when_test_call_uses_super_reports_violation() {
        let syntax = syn::parse_file(
            r"
            mod parent {
                fn helper() {}
                #[cfg(test)]
                mod tests {
                    use super::*;
                    fn run() {
                        super::helper();
                    }
                }
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![RelativePathViolation {
                location: Location::new(PathBuf::from("test.rs"), 8, 25),
            }])
        );
    }

    #[test]
    fn test_relative_path_check_when_block_import_uses_super_reports_violation() {
        let syntax = syn::parse_file(
            r"
            mod parent {
                fn helper() {}
                mod child {
                    fn run() {
                        use super::helper;
                    }
                }
            }
            ",
        )
        .unwrap();

        let result = RelativePathRule.check(&crate::cmds::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![RelativePathViolation {
                location: Location::new(PathBuf::from("test.rs"), 6, 29),
            }])
        );
    }
}
