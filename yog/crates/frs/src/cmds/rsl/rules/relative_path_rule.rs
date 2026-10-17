//! Report parent-module prefixes that need unavailable crate context.

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
use crate::cmds::rsl::ast::is_test_module_declaration;
use crate::cmds::rsl::engine::FileContext;
use crate::cmds::rsl::source::SourceEditor;

pub(super) struct RelativePathRule;

impl Rule for RelativePathRule {
    fn id(&self) -> &'static str {
        "relative-path"
    }

    fn fix(&self, ctx: &FileContext<'_>, editor: &mut SourceEditor<'_>) -> rootcause::Result<()> {
        let mut visitor = RelativePaths {
            test_module: false,
            prefixes: Vec::new(),
        };
        visitor.visit_file(ctx.file);

        for prefix in visitor.prefixes {
            if !editor.can_replace_path(prefix) {
                continue;
            }

            editor.skip("cannot resolve the parent module from this file");
        }

        Ok(())
    }
}

struct RelativePaths {
    test_module: bool,
    prefixes: Vec<Span>,
}

impl RelativePaths {
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

impl<'ast> Visit<'ast> for RelativePaths {
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
