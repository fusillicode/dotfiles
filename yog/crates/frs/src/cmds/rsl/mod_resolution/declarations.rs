//! Rust's module file layout, including path attributes and conditional alternatives.

use std::path::Path;
use std::path::PathBuf;

use syn::Item;
use syn::ItemMod;
use syn::Meta;
use syn::Token;
use syn::punctuated::Punctuated;
use syn::visit;
use syn::visit::Visit;

pub(super) struct ModuleDirectory {
    children: PathBuf,
    attributes: PathBuf,
    source: PathBuf,
}

impl ModuleDirectory {
    pub(super) fn root(directory: &Path) -> Self {
        Self {
            children: directory.to_owned(),
            attributes: directory.to_owned(),
            source: directory.to_owned(),
        }
    }

    pub(super) fn external_file(path: &Path, layout: FileLayout) -> Self {
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        let children = match layout {
            FileLayout::DirectoryOwner => parent.to_owned(),
            FileLayout::NamedFile if path.file_name().is_some_and(|name| name == "mod.rs") => parent.to_owned(),
            FileLayout::NamedFile => parent.join(path.file_stem().unwrap_or_default()),
        };

        Self {
            children,
            attributes: parent.to_owned(),
            source: parent.to_owned(),
        }
    }

    pub(super) fn inline(&self, module: &ItemMod) -> Vec<Self> {
        let paths = AttributePaths::collect(module);
        let mut directories: Vec<_> = paths
            .alternatives
            .iter()
            .map(|path| self.inline_directory(self.attributes.join(path)))
            .collect();
        if !paths.unconditional {
            directories.push(self.inline_directory(self.children.join(module.ident.to_string())));
        }

        directories
    }

    fn inline_directory(&self, directory: PathBuf) -> Self {
        Self {
            children: directory.clone(),
            attributes: directory,
            source: self.source.clone(),
        }
    }

    pub(super) fn include(&self, mac: &syn::Macro) -> Option<PathBuf> {
        let path = mac.parse_body::<syn::LitStr>().ok()?;

        Some(self.source.join(path.value()))
    }

    pub(super) fn external(&self, module: &ItemMod) -> Vec<ModuleFiles> {
        let paths = AttributePaths::collect(module);
        let mut files: Vec<_> = paths
            .alternatives
            .iter()
            .map(|path| ModuleFiles {
                candidates: vec![self.attributes.join(path)],
                layout: FileLayout::DirectoryOwner,
            })
            .collect();
        if !paths.unconditional {
            let name = module.ident.to_string();
            files.push(ModuleFiles {
                candidates: vec![
                    self.children.join(format!("{name}.rs")),
                    self.children.join(name).join("mod.rs"),
                ],
                layout: FileLayout::NamedFile,
            });
        }

        files
    }
}

pub(super) struct ModuleFiles {
    pub(super) candidates: Vec<PathBuf>,
    pub(super) layout: FileLayout,
}

#[derive(Clone, Copy)]
pub(super) enum FileLayout {
    DirectoryOwner,
    NamedFile,
}

pub(super) enum Declaration<'ast> {
    Module { item: &'ast ItemMod, local: bool },
    Include(&'ast syn::Macro),
    OpaqueExpansion,
}

impl<'ast> Declaration<'ast> {
    pub(super) fn collect(items: &'ast [Item]) -> Vec<Self> {
        let mut visitor = Declarations {
            modules: Vec::new(),
            local: false,
        };

        for item in items {
            visitor.visit_item(item);
        }

        visitor.modules
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ModuleStructure {
    Module {
        name: String,
        local: bool,
        paths: AttributePaths,
        children: Option<Vec<Self>>,
    },
    Include(String),
    OpaqueExpansion,
}

impl ModuleStructure {
    pub(super) fn collect(items: &[Item]) -> Vec<Self> {
        let mut structure: Vec<_> = Declaration::collect(items)
            .into_iter()
            .map(|declaration| match declaration {
                Declaration::Module { item, local } => Self::Module {
                    name: item.ident.to_string(),
                    local,
                    paths: AttributePaths::collect(item),
                    children: item.content.as_ref().map(|(_, items)| Self::collect(items)),
                },
                Declaration::Include(mac) => Self::Include(mac.tokens.to_string()),
                Declaration::OpaqueExpansion => Self::OpaqueExpansion,
            })
            .collect();

        structure.sort();

        structure
    }
}

struct Declarations<'ast> {
    modules: Vec<Declaration<'ast>>,
    local: bool,
}

impl<'ast> Visit<'ast> for Declarations<'ast> {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        self.modules.push(Declaration::Module {
            item: module,
            local: self.local,
        });
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("include") {
            self.modules.push(Declaration::Include(mac));
        }
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if item.ident.is_some() {
            return;
        }

        if item.mac.path.is_ident("include") {
            self.visit_macro(&item.mac);
        } else if !self.local {
            // An item macro can introduce additional modules or include existing source files.
            self.modules.push(Declaration::OpaqueExpansion);
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let previous = self.local;
        self.local = true;

        visit::visit_block(self, block);

        self.local = previous;
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct AttributePaths {
    alternatives: Vec<PathBuf>,
    unconditional: bool,
    pub(super) supported: bool,
}

impl AttributePaths {
    pub(super) fn collect(module: &ItemMod) -> Self {
        let mut paths = Self {
            alternatives: Vec::new(),
            unconditional: false,
            supported: true,
        };

        for attribute in &module.attrs {
            paths.meta(&attribute.meta, false);
        }

        paths
    }

    fn meta(&mut self, meta: &Meta, conditional: bool) {
        match meta {
            Meta::NameValue(value) if value.path.is_ident("path") => {
                if let syn::Expr::Lit(value) = &value.value
                    && let syn::Lit::Str(path) = &value.lit
                {
                    self.alternatives.push(PathBuf::from(path.value()));
                    self.unconditional |= !conditional;
                } else {
                    self.supported = false;
                }
            }
            Meta::List(list) if list.path.is_ident("cfg_attr") => {
                if let Ok(attributes) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
                    for attribute in attributes.iter().skip(1) {
                        self.meta(attribute, true);
                    }
                } else {
                    self.supported = false;
                }
            }
            Meta::Path(_) | Meta::List(_) | Meta::NameValue(_) => {
                let known = meta.path().get_ident().is_some_and(|name| {
                    matches!(
                        name.to_string().as_str(),
                        "cfg" | "doc" | "allow" | "warn" | "deny" | "forbid" | "expect" | "deprecated" | "macro_use"
                    )
                });
                let tool = meta.path().segments.first().is_some_and(|segment| {
                    matches!(
                        segment.ident.to_string().as_str(),
                        "rustfmt" | "clippy" | "rust_analyzer"
                    )
                });

                self.supported &= known || tool;
            }
        }
    }
}
