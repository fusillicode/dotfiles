#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use askama::Template;
use chrono::SecondsFormat;
use chrono::Utc;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;

// Embed minified CSS produced at build time so runtime does not depend on OUT_DIR.
const MINIFIED_STYLE_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.min.css"));

fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    cmd::silent_cmd("cargo")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .status()?
        .exit_ok()?;

    let workspace_root = get_workspace_root()?;

    let doc_dir = get_existing_doc_dir(&workspace_root)?;

    let mut crates = vec![];
    for crate_name in std::fs::read_dir(&doc_dir)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|file_type| file_type.is_dir()).unwrap_or(false))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(&doc_dir).join(name).join("index.html").is_file())
    {
        crates.push(CrateMeta {
            name: crate_name.clone(),
            description: get_crate_description(&workspace_root, &crate_name)?,
        })
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));

    let css_dest_path = doc_dir.join("style.css");
    std::fs::write(&css_dest_path, MINIFIED_STYLE_CSS)?;

    let tpl = Index {
        crates: &crates,
        generated: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false),
    };
    let html = tpl.render()?;

    let index_path = doc_dir.join("index.html");
    std::fs::create_dir_all(&doc_dir)?;
    std::fs::write(&index_path, html)?;

    Ok(())
}

#[derive(Template)]
#[template(path = "index.html")]
struct Index<'a> {
    crates: &'a [CrateMeta],
    generated: String,
}

struct CrateMeta {
    name: String,
    description: String,
}

fn get_workspace_root() -> color_eyre::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    Ok(manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or_eyre(format!(
            "cannot get workspace root from manifest_dir={}",
            manifest_dir.display()
        ))?
        .to_path_buf())
}

fn get_crate_description(workspace_root: &Path, crate_name: &str) -> color_eyre::Result<String> {
    let cargo_toml = dbg!(workspace_root.join("yog").join(crate_name).join("Cargo.toml"));
    let content = std::fs::read_to_string(cargo_toml)?;
    for line in content.lines() {
        let trimmed = line.trim_start();
        let Some(desc_line) = trimmed.strip_prefix("description") else {
            continue;
        };
        let &[_, desc] = desc_line.split('=').collect::<Vec<_>>().as_slice() else {
            bail!("description line does not have =, desc_line={desc_line}");
        };
        return Ok(desc.to_string());
    }
    bail!("no description line found in Cargo.toml for crate={crate_name}");
}

fn get_existing_doc_dir(workspace_root: &Path) -> color_eyre::Result<PathBuf> {
    let doc_dir = workspace_root.join("target/doc");
    if !doc_dir.exists() {
        bail!(
            "documentation directory '{}' does not exist; run 'cargo doc --workspace' first",
            doc_dir.display()
        )
    }
    Ok(doc_dir)
}
