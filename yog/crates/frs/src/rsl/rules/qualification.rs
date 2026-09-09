//! Qualification and import-style rule.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;

use proc_macro2::Span;
use serde::Serialize;
use syn::Expr;
use syn::Item;
use syn::PathArguments;
use syn::UseTree;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::rsl::engine::FileContext;
use crate::rsl::rules::TypedRule;
use crate::rsl::rules::TypedRuleViolation;

pub struct QualificationRule;

impl TypedRule for QualificationRule {
    type Violation = QualificationViolation;

    fn name() -> &'static str {
        "qualification"
    }

    fn check(&self, ctx: &FileContext<'_>) -> Vec<Self::Violation> {
        let index = self::module_index(ctx.file);
        let mut findings = Findings::default();

        for scope in &index.scopes {
            let mut visitor = QualificationVisitor {
                index: &index,
                current_module: &scope.path,
                source_path: ctx.path.to_path_buf(),
                findings: &mut findings,
                skip_call_path: false,
            };
            for item in scope.items {
                visitor.visit_item(item);
            }
        }

        let mut violations = Vec::new();
        violations.extend(findings.functions);
        violations.extend(findings.non_functions);
        violations.extend(findings.aliases);

        violations
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum QualificationViolation {
    Function(FunctionQualificationViolation),
    NonFunction(NonFunctionQualificationViolation),
    ForbiddenAlias(ForbiddenAliasViolation),
}

impl TypedRuleViolation for QualificationViolation {
    type Rule = QualificationRule;
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct FunctionQualificationViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: FunctionQualificationDetails,
}

impl FunctionQualificationViolation {
    fn new(path: &Path, span: Span, actual_path: String, expected_path: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "free function call is not properly qualified",
            details: FunctionQualificationDetails {
                actual_path,
                expected_path,
            },
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct NonFunctionQualificationViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: NonFunctionQualificationDetails,
}

impl NonFunctionQualificationViolation {
    fn new(path: &Path, span: Span, actual_path: String, expected_import: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "non-function item should be imported",
            details: NonFunctionQualificationDetails {
                actual_path,
                expected_import,
            },
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
pub(super) struct ForbiddenAliasViolation {
    file: PathBuf,
    line: usize,
    column: usize,
    message: &'static str,
    details: ForbiddenAliasDetails,
}

impl ForbiddenAliasViolation {
    fn new(path: &Path, span: Span, alias: String) -> Self {
        let location = span.start();
        Self {
            file: path.to_path_buf(),
            line: location.line,
            column: location.column.saturating_add(1),
            message: "import alias is forbidden",
            details: ForbiddenAliasDetails { alias },
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
struct FunctionQualificationDetails {
    actual_path: String,
    expected_path: String,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
struct NonFunctionQualificationDetails {
    actual_path: String,
    expected_import: String,
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
#[derive(Serialize)]
struct ForbiddenAliasDetails {
    alias: String,
}

struct ModuleScope<'ast> {
    path: Vec<String>,
    items: &'ast [Item],
}

#[derive(Default)]
struct ScopeInfo {
    definitions: HashSet<String>,
    functions: HashSet<String>,
    imports: Vec<ImportBinding>,
    unknown_imports: bool,
}

struct ImportBinding {
    name: String,
    path: Vec<String>,
}

struct ModuleIndex<'ast> {
    scopes: Vec<ModuleScope<'ast>>,
    info: HashMap<Vec<String>, ScopeInfo>,
    modules: HashSet<Vec<String>>,
}

#[derive(Default)]
struct Findings {
    functions: Vec<QualificationViolation>,
    non_functions: Vec<QualificationViolation>,
    aliases: Vec<QualificationViolation>,
}

struct QualificationVisitor<'index, 'ast, 'output> {
    index: &'index ModuleIndex<'ast>,
    current_module: &'index [String],
    source_path: PathBuf,
    findings: &'output mut Findings,
    skip_call_path: bool,
}

impl<'ast> Visit<'ast> for QualificationVisitor<'_, '_, '_> {
    fn visit_expr_call(&mut self, expression: &'ast syn::ExprCall) {
        if let Expr::Path(path) = expression.func.as_ref() {
            let expected_path = self::expected_function_path(self.index, self.current_module, &path.path);

            let actual_path = self::path_label(&path.path);
            if let Some(expected_path) = expected_path
                && actual_path != expected_path
            {
                self.findings
                    .functions
                    .push(QualificationViolation::Function(FunctionQualificationViolation::new(
                        &self.source_path,
                        path.span(),
                        actual_path,
                        expected_path,
                    )));
            }
        }

        let previous = self.skip_call_path;
        self.skip_call_path = matches!(expression.func.as_ref(), Expr::Path(_));
        syn::visit::visit_expr_call(self, expression);
        self.skip_call_path = previous;
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        if self.skip_call_path {
            self.skip_call_path = false;
        } else if path.segments.len() > 1
            && !self::is_self_path(path)
            && self::is_local_qualified_path(self.index, self.current_module, path)
            && !self::has_name_clash(self.index, self.current_module, path)
        {
            let actual_path = self::path_label(path);
            self.findings.non_functions.push(QualificationViolation::NonFunction(
                NonFunctionQualificationViolation::new(
                    &self.source_path,
                    path.span(),
                    actual_path,
                    format!("use {};", self::path_label(path)),
                ),
            ));
        }

        syn::visit::visit_path(self, path);
    }

    fn visit_item_mod(&mut self, _module: &'ast syn::ItemMod) {}

    fn visit_item_use(&mut self, item_use: &'ast syn::ItemUse) {
        self.check_aliases(&item_use.tree);
    }

    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {}
}

impl QualificationVisitor<'_, '_, '_> {
    fn check_aliases(&mut self, tree: &UseTree) {
        let mut pending = vec![tree];

        while let Some(tree) = pending.pop() {
            match tree {
                UseTree::Path(path) => pending.push(path.tree.as_ref()),
                UseTree::Group(group) => pending.extend(group.items.iter()),
                UseTree::Rename(rename) if rename.rename != "_" => {
                    self.findings
                        .aliases
                        .push(QualificationViolation::ForbiddenAlias(ForbiddenAliasViolation::new(
                            &self.source_path,
                            rename.rename.span(),
                            rename.rename.to_string(),
                        )));
                }
                UseTree::Name(_) | UseTree::Glob(_) | UseTree::Rename(_) => {}
            }
        }
    }
}

fn module_index(file: &syn::File) -> ModuleIndex<'_> {
    let mut index = ModuleIndex {
        scopes: Vec::new(),
        info: HashMap::new(),
        modules: HashSet::new(),
    };
    let mut pending = VecDeque::from([(Vec::new(), file.items.as_slice())]);

    while let Some((path, items)) = pending.pop_front() {
        let mut info = ScopeInfo::default();
        for item in items {
            if let Some(name) = self::item_name(item) {
                info.definitions.insert(name);
            }
            if let Item::Fn(function) = item {
                info.functions.insert(function.sig.ident.to_string());
            }
            if let Item::Mod(module) = item {
                let mut nested_path = path.clone();
                nested_path.push(module.ident.to_string());
                index.modules.insert(nested_path.clone());
                if let Some((_, nested_items)) = &module.content {
                    pending.push_back((nested_path, nested_items.as_slice()));
                }
            }
            if let Item::Use(item_use) = item {
                let bindings = self::use_bindings(&item_use.tree, &path);
                info.imports.extend(bindings.bindings);
                info.unknown_imports |= bindings.unknown;
            }
        }
        index.info.insert(path.clone(), info);
        index.scopes.push(ModuleScope { path, items });
    }

    index
}

fn item_name(item: &Item) -> Option<String> {
    match item {
        Item::Const(item) => Some(item.ident.to_string()),
        Item::Enum(item) => Some(item.ident.to_string()),
        Item::ExternCrate(item) => Some(item.ident.to_string()),
        Item::Fn(item) => Some(item.sig.ident.to_string()),
        Item::Mod(item) => Some(item.ident.to_string()),
        Item::Static(item) => Some(item.ident.to_string()),
        Item::Struct(item) => Some(item.ident.to_string()),
        Item::Trait(item) => Some(item.ident.to_string()),
        Item::TraitAlias(item) => Some(item.ident.to_string()),
        Item::Type(item) => Some(item.ident.to_string()),
        Item::Union(item) => Some(item.ident.to_string()),
        Item::ForeignMod(_) | Item::Impl(_) | Item::Macro(_) | Item::Use(_) | Item::Verbatim(_) | _ => None,
    }
}

#[derive(Default)]
struct UseBindings {
    bindings: Vec<ImportBinding>,
    unknown: bool,
}

fn use_bindings(tree: &UseTree, current_module: &[String]) -> UseBindings {
    let mut bindings = UseBindings::default();
    let mut pending = vec![(tree, Vec::<String>::new())];

    while let Some((tree, prefix)) = pending.pop() {
        match tree {
            UseTree::Path(path) => {
                let mut next_prefix = prefix;
                next_prefix.push(path.ident.to_string());
                pending.push((path.tree.as_ref(), next_prefix));
            }
            UseTree::Group(group) => {
                pending.extend(group.items.iter().map(|tree| (tree, prefix.clone())));
            }
            UseTree::Name(name) => {
                if name.ident == "self" {
                    if let Some(binding) = prefix.last() {
                        bindings.bindings.push(ImportBinding {
                            name: binding.clone(),
                            path: self::normalize_path(current_module, &prefix),
                        });
                    }
                } else {
                    let mut path = prefix;
                    path.push(name.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: name.ident.to_string(),
                        path: self::normalize_path(current_module, &path),
                    });
                }
            }
            UseTree::Rename(rename) => {
                if rename.rename != "_" {
                    let mut path = prefix;
                    path.push(rename.ident.to_string());
                    bindings.bindings.push(ImportBinding {
                        name: rename.rename.to_string(),
                        path: self::normalize_path(current_module, &path),
                    });
                }
            }
            UseTree::Glob(_) => bindings.unknown = true,
        }
    }

    bindings
}

fn path_parts(path: &syn::Path) -> Option<Vec<String>> {
    if path.leading_colon.is_some() || path.segments.is_empty() {
        return None;
    }

    let last_segment = path.segments.len().saturating_sub(1);
    let mut parts = Vec::new();
    for (index, segment) in path.segments.iter().enumerate() {
        if index != last_segment && !matches!(&segment.arguments, PathArguments::None) {
            return None;
        }
        parts.push(segment.ident.to_string());
    }
    Some(parts)
}

fn expected_foreign_call_path(index: &ModuleIndex<'_>, current_module: &[String], path: &syn::Path) -> Option<String> {
    let parts = self::path_parts(path)?;
    if parts.len() <= 1 {
        return self::expected_imported_call_path(index, current_module, &parts);
    }

    let mut base = current_module.to_vec();
    let mut first_item = 0;
    match parts.first()?.as_str() {
        "crate" => return None,
        "self" => {
            first_item = 1;
            let module = parts.get(first_item)?;
            if !self::is_direct_module_name(index, current_module, module) {
                return self::expected_imported_call_path(index, current_module, &parts);
            }
        }
        "super" => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                base.pop()?;
                first_item = first_item.saturating_add(1);
            }
            let module = parts.get(first_item)?;
            if !self::is_direct_module_name(index, &base, module) {
                return None;
            }
        }
        _ => {
            let module = parts.first()?;
            if !self::is_direct_module_name(index, current_module, module) {
                return self::expected_imported_call_path(index, current_module, &parts);
            }
        }
    }

    let mut absolute_parts = base;
    absolute_parts.extend(parts.get(first_item..)?.iter().cloned());
    Some(self::crate_path_segments(&absolute_parts))
}

fn expected_imported_call_path(index: &ModuleIndex<'_>, current_module: &[String], parts: &[String]) -> Option<String> {
    let (binding_name, first_item) = match parts.first()?.as_str() {
        "self" => (parts.get(1)?, 2),
        _ => (parts.first()?, 1),
    };
    let binding = self::imported_binding(index, current_module, binding_name)?;
    let source_item = binding.path.last()?;
    let source_module = binding.path.get(..binding.path.len().saturating_sub(1))?;
    if !self::is_local_module_path(index, source_module) {
        return None;
    }

    let mut suffix = vec![source_item.clone()];
    suffix.extend(parts.get(first_item..)?.iter().cloned());
    if source_module == current_module {
        return Some(format!("self::{}", suffix.join("::")));
    }

    let mut absolute_parts = source_module.to_vec();
    absolute_parts.extend(suffix);
    Some(self::crate_path_segments(&absolute_parts))
}

fn has_name_clash(index: &ModuleIndex<'_>, current_module: &[String], path: &syn::Path) -> bool {
    let Some(scope) = index.info.get(current_module) else {
        return true;
    };
    if scope.unknown_imports {
        return true;
    }

    let Some(parts) = self::path_parts(path) else {
        return true;
    };
    let Some(name) = parts.last() else {
        return true;
    };
    let Some(target_module_parts) = parts.get(..parts.len().saturating_sub(1)) else {
        return true;
    };
    let target_module = self::normalize_path(current_module, target_module_parts);
    let target_path = self::normalize_path(current_module, &parts);

    if target_module != current_module && scope.definitions.contains(name) {
        return true;
    }

    scope
        .imports
        .iter()
        .any(|binding| binding.name == *name && binding.path != target_path)
}

fn is_local_qualified_path(index: &ModuleIndex<'_>, current_module: &[String], path: &syn::Path) -> bool {
    match path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
        .as_deref()
    {
        Some("crate" | "self" | "super") => true,
        Some(name) => self::is_direct_module_name(index, current_module, name),
        None => false,
    }
}

fn expected_function_path(index: &ModuleIndex<'_>, current_module: &[String], path: &syn::Path) -> Option<String> {
    if let Some((module_path, name)) = self::local_function(index, current_module, path) {
        return Some(if module_path == current_module {
            format!("self::{name}")
        } else {
            self::crate_path(&module_path, &name)
        });
    }

    self::expected_foreign_call_path(index, current_module, path)
}

fn local_function(
    index: &ModuleIndex<'_>,
    current_module: &[String],
    path: &syn::Path,
) -> Option<(Vec<String>, String)> {
    let parts = self::path_parts(path)?;
    let name = parts.last()?.clone();
    let last_item = parts.len().saturating_sub(1);
    let mut module_path = current_module.to_vec();
    let mut first_item = 0;

    match parts.first()?.as_str() {
        "crate" => {
            module_path.clear();
            first_item = 1;
        }
        "self" => first_item = 1,
        "super" => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                module_path.pop()?;
                first_item = first_item.saturating_add(1);
            }
        }
        _ => {}
    }

    if first_item > last_item {
        return None;
    }
    module_path.extend(parts.get(first_item..last_item)?.iter().cloned());
    if !self::is_local_module_path(index, &module_path) {
        return None;
    }

    let scope = index.info.get(&module_path)?;
    if scope.unknown_imports || scope.imports.iter().any(|binding| binding.name == name) {
        return None;
    }
    scope.functions.contains(&name).then_some((module_path, name))
}

fn is_local_module_path(index: &ModuleIndex<'_>, path: &[String]) -> bool {
    path.is_empty() || index.modules.contains(path)
}

fn is_direct_module_name(index: &ModuleIndex<'_>, current_module: &[String], name: &str) -> bool {
    let mut path = current_module.to_vec();
    path.push(name.to_owned());
    index.modules.contains(&path)
        && index
            .info
            .get(current_module)
            .is_some_and(|scope| !scope.imports.iter().any(|binding| binding.name == name))
}

fn imported_binding<'index>(
    index: &'index ModuleIndex<'_>,
    current_module: &[String],
    name: &str,
) -> Option<&'index ImportBinding> {
    let scope = index.info.get(current_module)?;
    if scope.unknown_imports {
        return None;
    }
    let mut bindings = scope.imports.iter().filter(|binding| binding.name == name);
    let binding = bindings.next()?;
    bindings.next().is_none().then_some(binding)
}

fn crate_path(module_path: &[String], name: &str) -> String {
    let mut parts = Vec::with_capacity(module_path.len().saturating_add(2));
    parts.push("crate".to_owned());
    parts.extend(module_path.iter().cloned());
    parts.push(name.to_owned());
    parts.join("::")
}

fn crate_path_segments(segments: &[String]) -> String {
    let mut parts = Vec::with_capacity(segments.len().saturating_add(1));
    parts.push("crate".to_owned());
    parts.extend(segments.iter().cloned());
    parts.join("::")
}

fn normalize_path(current_module: &[String], parts: &[String]) -> Vec<String> {
    let mut absolute_parts = current_module.to_vec();
    let mut first_item = 0;

    match parts.first().map(String::as_str) {
        Some("crate") => {
            absolute_parts.clear();
            first_item = 1;
        }
        Some("self") => first_item = 1,
        Some("super") => {
            while parts.get(first_item).is_some_and(|part| part == "super") {
                if absolute_parts.pop().is_none() {
                    return Vec::new();
                }
                first_item = first_item.saturating_add(1);
            }
        }
        Some(_) | None => {}
    }

    if let Some(suffix) = parts.get(first_item..) {
        absolute_parts.extend(suffix.iter().cloned());
    }
    absolute_parts
}

fn is_self_path(path: &syn::Path) -> bool {
    path.segments.first().is_some_and(|segment| segment.ident == "Self")
}

fn path_label(path: &syn::Path) -> String {
    self::path_parts(path).map_or_else(|| "qualified path".to_owned(), |parts| parts.join("::"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use test_that::prelude::*;

    use super::ForbiddenAliasDetails;
    use super::ForbiddenAliasViolation;
    use super::FunctionQualificationDetails;
    use super::FunctionQualificationViolation;
    use super::NonFunctionQualificationDetails;
    use super::NonFunctionQualificationViolation;
    use super::QualificationRule;
    use super::QualificationViolation;
    use crate::rsl::rules::TypedRule;

    #[test]
    fn test_qualification_rule_check_when_same_module_call_is_bare_reports_call() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                helper();
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::Function(FunctionQualificationViolation {
                file: PathBuf::from("test.rs"),
                line: 4,
                column: 17,
                message: "free function call is not properly qualified",
                details: FunctionQualificationDetails {
                    actual_path: "helper".to_owned(),
                    expected_path: "self::helper".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_same_module_call_uses_self_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            fn helper() {}
            fn run() {
                self::helper();
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_imported_foreign_call_is_bare_reports_call() {
        let syntax = syn::parse_file(
            r"
            mod external;
            use external::run;
            fn main() {
                run();
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::Function(FunctionQualificationViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 17,
                message: "free function call is not properly qualified",
                details: FunctionQualificationDetails {
                    actual_path: "run".to_owned(),
                    expected_path: "crate::external::run".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_foreign_module_call_is_relative_reports_call() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                helper::run();
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::Function(FunctionQualificationViolation {
                file: PathBuf::from("test.rs"),
                line: 6,
                column: 17,
                message: "free function call is not properly qualified",
                details: FunctionQualificationDetails {
                    actual_path: "helper::run".to_owned(),
                    expected_path: "crate::helper::run".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_foreign_module_call_uses_crate_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            mod helper {
                pub fn run() {}
            }
            fn main() {
                crate::helper::run();
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_non_function_path_is_fully_qualified_reports_import() {
        let syntax = syn::parse_file(
            r"
            mod values {
                pub const VALUE: usize = 1;
            }
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::NonFunction(
                NonFunctionQualificationViolation {
                    file: PathBuf::from("test.rs"),
                    line: 6,
                    column: 17,
                    message: "non-function item should be imported",
                    details: NonFunctionQualificationDetails {
                        actual_path: "crate::values::VALUE".to_owned(),
                        expected_import: "use crate::values::VALUE;".to_owned(),
                    },
                }
            )])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_non_function_name_clashes_allows_qualified_path() {
        let syntax = syn::parse_file(
            r"
            mod values {
                pub const VALUE: usize = 1;
            }
            const VALUE: usize = 2;
            fn read() -> usize {
                crate::values::VALUE
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_struct_names_clash_allows_one_qualified_path() {
        let syntax = syn::parse_file(
            r"
            mod first {
                pub struct Thing;
            }
            mod second {
                pub struct Thing;
            }
            use crate::first::Thing;
            fn read() -> crate::second::Thing {
                panic!()
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_alias_is_private_reports_alias() {
        let syntax = syn::parse_file(
            r"
            use std::fmt::Display as Formatter;
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::ForbiddenAlias(ForbiddenAliasViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 38,
                message: "import alias is forbidden",
                details: ForbiddenAliasDetails {
                    alias: "Formatter".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_alias_is_wildcard_returns_no_violations() {
        let syntax = syn::parse_file(
            r"
            use std::fmt::Display as _;
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_public_reexport_is_renamed_reports_alias() {
        let syntax = syn::parse_file(
            r"
            pub use std::fmt::Debug as Formatter;
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::ForbiddenAlias(ForbiddenAliasViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 40,
                message: "import alias is forbidden",
                details: ForbiddenAliasDetails {
                    alias: "Formatter".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_reexport_alias_is_restricted_reports_alias() {
        let syntax = syn::parse_file(
            r"
            pub(crate) use std::fmt::Debug as Formatter;
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::ForbiddenAlias(ForbiddenAliasViolation {
                file: PathBuf::from("test.rs"),
                line: 2,
                column: 47,
                message: "import alias is forbidden",
                details: ForbiddenAliasDetails {
                    alias: "Formatter".to_owned(),
                },
            })])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_external_paths_are_qualified_reports_paths() {
        let syntax = syn::parse_file(
            r"
            mod external;
            use external::Thing;
            struct Data;
            impl Data {
                fn run() {}
                fn call(&self) {
                    self.run();
                    Self::run();
                }
            }
            fn read() -> external::Thing {
                external::Thing::new()
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![
                QualificationViolation::Function(FunctionQualificationViolation {
                    file: PathBuf::from("test.rs"),
                    line: 13,
                    column: 17,
                    message: "free function call is not properly qualified",
                    details: FunctionQualificationDetails {
                        actual_path: "external::Thing::new".to_owned(),
                        expected_path: "crate::external::Thing::new".to_owned(),
                    },
                }),
                QualificationViolation::NonFunction(NonFunctionQualificationViolation {
                    file: PathBuf::from("test.rs"),
                    line: 12,
                    column: 26,
                    message: "non-function item should be imported",
                    details: NonFunctionQualificationDetails {
                        actual_path: "external::Thing".to_owned(),
                        expected_import: "use external::Thing;".to_owned(),
                    },
                }),
            ])
        );
    }

    #[test]
    fn test_qualification_rule_check_when_unknown_external_type_is_qualified_ignores_path() {
        let syntax = syn::parse_file(
            r"
            fn inspect(_: syn::ExprCall) {}
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_enum_variant_is_qualified_ignores_path() {
        let syntax = syn::parse_file(
            r"
            enum Kind {
                First,
                Second,
            }
            fn select(kind: Kind) -> Kind {
                match kind {
                    Kind::First => Kind::Second,
                    Kind::Second => Kind::First,
                }
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_associated_function_is_referenced_ignores_path() {
        let syntax = syn::parse_file(
            r"
            fn converter() -> fn(&String) -> &str {
                String::as_str
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(result, empty());
    }

    #[test]
    fn test_qualification_rule_check_when_nested_module_call_is_bare_reports_nested_call() {
        let syntax = syn::parse_file(
            r"
            mod outer {
                fn helper() {}
                fn run() {
                    helper();
                }
            }
            ",
        )
        .unwrap();

        let result = QualificationRule.check(&crate::rsl::rules::test_ctx(&syntax));

        assert_that!(
            result,
            eq(vec![QualificationViolation::Function(FunctionQualificationViolation {
                file: PathBuf::from("test.rs"),
                line: 5,
                column: 21,
                message: "free function call is not properly qualified",
                details: FunctionQualificationDetails {
                    actual_path: "helper".to_owned(),
                    expected_path: "self::helper".to_owned(),
                },
            })])
        );
    }
}
