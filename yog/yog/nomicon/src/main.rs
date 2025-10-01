//! Generate a consolidated styled workspace documentation.
#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use askama::Template;
use chrono::Utc;
use color_eyre::eyre::bail;

use crate::templates::components::footer::Footer;
use crate::templates::pages::index::CrateMeta;
use crate::templates::pages::index::IndexPage;
use crate::templates::pages::not_found::NotFoundPage;

mod templates;

/// Generate a custom workspace documentation by wrapping `cargo doc` and
/// producing a unified landing page linking to all crates generated docs.
///
/// # Usage
///
/// ```bash
/// # From any workspace directory
/// nomicon
/// # Afterwards open target/doc/index.html (or serve) for aggregated view
/// ```
///
/// # Errors
/// If:
/// - `cargo doc` invocation fails or exits non‑zero.
/// - Workspace root or documentation directory cannot be resolved.
/// - A crate manifest cannot be read or parsed for required keys.
/// - A template fails to render.
/// - Writing output files or copying assets fails.
fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    // Always (re)generate docs for all workspace crates (including private items) first.
    // Use RUSTDOCFLAGS to enforce warnings-as-errors (portable across cargo versions).
    ytil_cmd::silent_cmd("cargo")
        .env("RUSTDOCFLAGS", "-Dwarnings")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .status()?
        .exit_ok()?;

    let workspace_root = ytil_system::get_workspace_root()?;
    let doc_dir = get_existing_doc_dir(&workspace_root)?;
    let cargo_tomls = ytil_system::find_matching_files_recursively_in_dir(
        &workspace_root,
        |entry| entry.path().file_name().is_some_and(|f| f == "Cargo.toml"),
        |entry| {
            let dir_name = entry.file_name();
            let dir_name = dir_name.to_string_lossy();
            dir_name.starts_with('.') || dir_name == "target" || dir_name == "node_modules"
        },
    )?;

    let mut crates = Vec::new();
    for cargo_toml in cargo_tomls {
        // Skip the workspace root Cargo.toml if it lacks a [package] section.
        let content = std::fs::read_to_string(&cargo_toml)?;
        if !content
            .lines()
            .next()
            .is_some_and(|l| l.trim_start().starts_with("[package]"))
        {
            continue;
        }

        let name = get_toml_value(&content, "name")?;
        let description = get_toml_value(&content, "description")?;

        // Only include crates that actually have a generated index (documentation produced).
        let index_html = doc_dir.join(&name).join("index.html");
        if index_html.is_file() {
            crates.push(CrateMeta { name, description });
        }
    }
    crates.sort_by(|a, b| a.name.cmp(&b.name));

    let generated_at = Utc::now();
    let footer = Footer { generated_at };

    let index_page = IndexPage {
        crates: &crates,
        footer: footer.clone(),
    };
    std::fs::write(doc_dir.join("index.html"), index_page.render()?)?;

    let not_found_page = NotFoundPage { footer }.render()?;
    std::fs::write(doc_dir.join("not_found.html"), not_found_page)?;

    copy_assets(&doc_dir)?;

    Ok(())
}

/// Get existing documentation directory if exists.
///
/// # Arguments
/// * `workspace_root` - Workspace root path.
///
/// # Returns
/// Absolute docs directory path.
///
/// # Errors
/// If:
/// - The directory is missing (suggest running `cargo doc --workspace`).
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

/// Copy static assets into documentation output directory.
///
/// Copies all files under crate-local `assets/` (CSS, fonts, favicon) into
/// `<workspace_root>/target/doc/assets` using a recursive `cp`.
///
/// # Arguments
/// * `doc_dir` - Existing `<workspace>/target/doc` directory.
///
/// # Returns
/// Ok on success.
///
/// # Errors
/// If:
/// - Underlying `cp` command execution fails.
/// - Destination directory cannot be written.
fn copy_assets(doc_dir: &Path) -> color_eyre::Result<()> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    let dest = doc_dir.join("assets");
    ytil_cmd::silent_cmd("cp")
        .args(["-r", &source.to_string_lossy(), &dest.to_string_lossy()])
        .status()?
        .exit_ok()?;
    Ok(())
}

/// Extract the value of the supplied `key` from the supplied manifest text `content`.
///
/// # Arguments
/// * `content` - Toml file content.
/// * `key` - Key name to search (e.g. "name").
///
/// # Returns
/// Value with surrounding quotes removed if present.
///
/// # Errors
/// If:
/// - The matching line is malformed (missing '=' or value).
/// - The key is not present.
fn get_toml_value(content: &str, key: &str) -> color_eyre::Result<String> {
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
