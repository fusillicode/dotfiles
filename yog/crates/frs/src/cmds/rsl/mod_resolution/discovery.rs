//! Resolve source declarations into module descriptions before assigning graph identities.
//!
//! Only the source cache performs shared mutation. Each traversal returns its own resolution outcomes,
//! including uncertainty, so reading a file cannot silently change the graph under construction.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use syn::File;
use syn::Item;
use syn::ItemMod;

use super::SourceFile;
use super::declarations::AttributePaths;
use super::declarations::Declaration;
use super::declarations::FileLayout;
use super::declarations::ModuleDirectory;
use super::declarations::ModuleFiles;
use super::declarations::ModuleStructure;

pub(super) struct DiscoveryOutput {
    pub(super) roots: Vec<ModuleResolution>,
    pub(super) structures: HashMap<PathBuf, Vec<ModuleStructure>>,
}

pub(super) fn resolve_roots(
    roots: &[PathBuf],
    read: impl FnMut(&Path) -> io::Result<SourceFile>,
) -> io::Result<DiscoveryOutput> {
    let mut sources = SourceCache {
        parsed: HashMap::new(),
        aliases: HashMap::new(),
        read,
    };

    let placement = ModulePlacement::CrateRoot;
    let roots = roots
        .iter()
        .filter_map(|path| path.parent().map(|parent| (path, parent)))
        .map(|(path, parent)| resolve_file(path, &placement, &ModuleDirectory::root(parent), &mut sources, &[]))
        .collect::<io::Result<_>>()?;

    let structures = sources
        .parsed
        .values()
        .filter_map(|source| match source.as_ref() {
            ParsedSource::File { path, file } => Some((path.clone(), ModuleStructure::collect(&file.items))),
            ParsedSource::Invalid(_) => None,
        })
        .collect();

    Ok(DiscoveryOutput { roots, structures })
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) enum ModuleResolution {
    Module {
        description: ModuleDescription,
        children: Vec<Self>,
    },
    UnresolvedFiles(Vec<PathBuf>),
    UnexpandedModules,
    Missing,
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct ModuleDescription {
    pub(super) placement: ModulePlacement,
    pub(super) file: PathBuf,
    pub(super) inline_modules: Vec<String>,
}

impl ModuleDescription {
    fn inline(&self, placement: ModulePlacement) -> Self {
        let mut inline_modules = self.inline_modules.clone();
        if let ModulePlacement::Declared { name, .. } = &placement {
            inline_modules.push(name.clone());
        }

        Self {
            placement,
            file: self.file.clone(),
            inline_modules,
        }
    }
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) enum ModulePlacement {
    CrateRoot,
    Declared { name: String, addressable: bool },
    Included,
}

impl ModulePlacement {
    pub(super) const fn is_addressable(&self) -> bool {
        match self {
            Self::CrateRoot => true,
            Self::Declared { addressable, .. } => *addressable,
            Self::Included => false,
        }
    }
}

fn resolve_file(
    path: &Path,
    placement: &ModulePlacement,
    directory: &ModuleDirectory,
    sources: &mut SourceCache<impl FnMut(&Path) -> io::Result<SourceFile>>,
    ancestors: &[PathBuf],
) -> io::Result<ModuleResolution> {
    let loaded = match sources.load(path) {
        Ok(loaded) => loaded,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => return Ok(ModuleResolution::Missing),
        Err(error) => return Err(error),
    };

    let Some(source) = loaded else {
        return Ok(ModuleResolution::Missing);
    };

    let (path, file) = match source.as_ref() {
        ParsedSource::File { path, file } => (path, file),
        ParsedSource::Invalid(path) => return Ok(ModuleResolution::UnresolvedFiles(vec![path.clone()])),
    };
    if ancestors.contains(path) {
        return Ok(ModuleResolution::UnresolvedFiles(ancestors.to_owned()));
    }

    let description = ModuleDescription {
        placement: placement.clone(),
        file: path.clone(),
        inline_modules: Vec::new(),
    };
    let ancestors: Vec<_> = ancestors.iter().cloned().chain(std::iter::once(path.clone())).collect();
    let children = resolve_declarations(&description, &file.items, directory, sources, &ancestors)?;

    Ok(ModuleResolution::Module { description, children })
}

fn resolve_declarations(
    parent: &ModuleDescription,
    items: &[Item],
    directory: &ModuleDirectory,
    sources: &mut SourceCache<impl FnMut(&Path) -> io::Result<SourceFile>>,
    ancestors: &[PathBuf],
) -> io::Result<Vec<ModuleResolution>> {
    let resolutions = Declaration::collect(items)
        .into_iter()
        .map(|declaration| match declaration {
            Declaration::Module { item, local } => resolve_module(parent, item, local, directory, sources, ancestors),
            Declaration::Include(mac) => resolve_include(mac, directory, sources, ancestors),
        })
        .collect::<io::Result<Vec<_>>>()?;

    Ok(resolutions.into_iter().flatten().collect())
}

fn resolve_module(
    parent: &ModuleDescription,
    module: &ItemMod,
    local: bool,
    directory: &ModuleDirectory,
    sources: &mut SourceCache<impl FnMut(&Path) -> io::Result<SourceFile>>,
    ancestors: &[PathBuf],
) -> io::Result<Vec<ModuleResolution>> {
    let attributes_supported = AttributePaths::collect(module).supported;
    let placement = ModulePlacement::Declared {
        name: module.ident.to_string(),
        addressable: parent.placement.is_addressable() && !local && attributes_supported,
    };

    let mut resolutions = if let Some((_, items)) = &module.content {
        let description = parent.inline(placement);

        directory
            .inline(module)
            .into_iter()
            .map(|directory| {
                let children = resolve_declarations(&description, items, &directory, sources, ancestors)?;

                Ok(ModuleResolution::Module {
                    description: description.clone(),
                    children,
                })
            })
            .collect::<io::Result<Vec<_>>>()?
    } else {
        let groups = directory
            .external(module)
            .into_iter()
            .map(|files| resolve_candidates(files, &placement, sources, ancestors))
            .collect::<io::Result<Vec<_>>>()?;

        groups.into_iter().flatten().collect()
    };

    if !attributes_supported {
        resolutions.insert(0, ModuleResolution::UnexpandedModules);
    }

    Ok(resolutions)
}

fn resolve_candidates(
    files: ModuleFiles,
    placement: &ModulePlacement,
    sources: &mut SourceCache<impl FnMut(&Path) -> io::Result<SourceFile>>,
    ancestors: &[PathBuf],
) -> io::Result<Vec<ModuleResolution>> {
    let loaded = files
        .candidates
        .into_iter()
        .map(|path| match sources.load(&path) {
            Ok(source) => Ok(source.map(|source| (path, source))),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(None),
            Err(error) => Err(error),
        })
        .collect::<io::Result<Vec<_>>>()?;

    let found: Vec<_> = loaded.into_iter().flatten().collect();
    let valid_count = found
        .iter()
        .filter(|(_, source)| matches!(source.as_ref(), ParsedSource::File { .. }))
        .count();

    let placement = match placement {
        ModulePlacement::Declared { name, addressable } => ModulePlacement::Declared {
            name: name.clone(),
            addressable: *addressable && valid_count <= 1,
        },
        ModulePlacement::CrateRoot => ModulePlacement::CrateRoot,
        ModulePlacement::Included => ModulePlacement::Included,
    };

    let outcomes = found
        .into_iter()
        .map(|(requested, source)| match source.as_ref() {
            ParsedSource::Invalid(path) => Ok(vec![ModuleResolution::UnresolvedFiles(vec![path.clone()])]),
            ParsedSource::File { path, .. } => {
                let resolution = resolve_file(
                    &requested,
                    &placement,
                    &ModuleDirectory::external_file(&requested, files.layout),
                    sources,
                    ancestors,
                )?;

                if valid_count > 1 {
                    Ok(vec![ModuleResolution::UnresolvedFiles(vec![path.clone()]), resolution])
                } else {
                    Ok(vec![resolution])
                }
            }
        })
        .collect::<io::Result<Vec<_>>>()?;

    Ok(outcomes.into_iter().flatten().collect())
}

fn resolve_include(
    mac: &syn::Macro,
    directory: &ModuleDirectory,
    sources: &mut SourceCache<impl FnMut(&Path) -> io::Result<SourceFile>>,
    ancestors: &[PathBuf],
) -> io::Result<Vec<ModuleResolution>> {
    let Some(path) = directory.include(mac) else {
        return Ok(vec![ModuleResolution::UnexpandedModules]);
    };

    let loaded = match sources.load(&path) {
        Ok(loaded) => loaded,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return Ok(vec![ModuleResolution::UnexpandedModules]);
        }
        Err(error) => return Err(error),
    };

    let Some(source) = loaded else {
        return Ok(vec![ModuleResolution::Missing]);
    };

    if let ParsedSource::Invalid(path) = source.as_ref() {
        return Ok(vec![
            ModuleResolution::UnresolvedFiles(vec![path.clone()]),
            ModuleResolution::UnexpandedModules,
        ]);
    }

    // Included tokens inherit their caller's scope. Explore both file layouts without claiming an absolute path.
    [FileLayout::DirectoryOwner, FileLayout::NamedFile]
        .into_iter()
        .map(|layout| {
            resolve_file(
                &path,
                &ModulePlacement::Included,
                &ModuleDirectory::external_file(&path, layout),
                sources,
                ancestors,
            )
        })
        .collect()
}

struct SourceCache<Reader> {
    parsed: HashMap<PathBuf, Rc<ParsedSource>>,
    aliases: HashMap<PathBuf, PathBuf>,
    read: Reader,
}

impl<Reader> SourceCache<Reader>
where
    Reader: FnMut(&Path) -> io::Result<SourceFile>,
{
    fn load(&mut self, path: &Path) -> io::Result<Option<Rc<ParsedSource>>> {
        let canonical = self.aliases.get(path).map_or(path, PathBuf::as_path);
        if let Some(source) = self.parsed.get(canonical) {
            return Ok(Some(Rc::clone(source)));
        }

        let source = match (self.read)(path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };

        let canonical = source.path.clone();
        let parsed = self
            .parsed
            .entry(canonical.clone())
            .or_insert_with(|| Rc::new(parse_source(source)));
        self.aliases.insert(path.to_owned(), canonical);

        Ok(Some(Rc::clone(parsed)))
    }
}

enum ParsedSource {
    File { path: PathBuf, file: File },
    Invalid(PathBuf),
}

fn parse_source(source: SourceFile) -> ParsedSource {
    match syn::parse_file(&source.text) {
        Ok(file) => ParsedSource::File {
            path: source.path,
            file,
        },
        Err(_) => ParsedSource::Invalid(source.path),
    }
}
