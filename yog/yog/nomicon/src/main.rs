#![feature(exit_status_error)]

use std::collections::VecDeque;
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

    // Always (re)generate docs for all workspace crates (including private items) first.
    ytil_cmd::silent_cmd("cargo")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .status()?
        .exit_ok()?;

    let workspace_root = get_workspace_root()?;
    let doc_dir = get_existing_doc_dir(&workspace_root)?;

    let manifests = find_all_manifests(&workspace_root)?;
    let mut crates = Vec::new();

    for manifest in manifests {
        // Skip the workspace root Cargo.toml if it lacks a [package] section.
        let content = std::fs::read_to_string(&manifest)?;
        if !content.lines().any(|l| l.trim_start().starts_with("[package]")) {
            continue;
        }

        let name = get_cargo_toml_key_value(&content, "name")?;
        let description = get_cargo_toml_key_value(&content, "description")?;

        // Only include crates that actually have a generated index (documentation produced).
        let index_html = doc_dir.join(&name).join("index.html");
        if index_html.is_file() {
            crates.push(CrateMeta { name, description });
        }
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

/// Recursively discover all Cargo.toml manifests under the workspace root.
fn find_all_manifests(workspace_root: &Path) -> color_eyre::Result<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    let mut queue = VecDeque::from([workspace_root.to_path_buf()]);

    while let Some(dir) = queue.pop_front() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
                queue.push_back(path);
            } else if path.file_name().is_some_and(|f| f == "Cargo.toml") {
                manifests.push(path);
            }
        }
    }

    Ok(manifests)
}

/// Return the value for the first `key = value` line.
///
/// Errors if the key is missing or malformed.
fn get_cargo_toml_key_value(content: &str, key: &str) -> color_eyre::Result<String> {
    for line in content.lines() {
        let trimmed_line = line.trim_start();
        if let Some(rest) = trimmed_line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                return Ok(after_eq.trim().trim_matches('"').to_string());
            }
            bail!("malformed key line for '{key}': {line}");
        }
    }
    bail!("required key '{key}' missing in manifest");
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
