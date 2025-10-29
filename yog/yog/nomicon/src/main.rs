//! Generate consolidated styled workspace documentation.
//!
//! # Errors
//! - Removing existing docs directory fails (other than `NotFound`).
//! - `cargo doc` exits non-zero.
//! - Reading a `Cargo.toml` or extracting required keys fails.
//! - Template rendering fails.
//! - Writing output files or copying static assets fails.
//! - UTF-8 conversion or metadata parsing fails.

#![feature(exit_status_error)]

use std::io::ErrorKind::NotFound;
use std::path::Path;
use std::path::PathBuf;

use askama::Template;
use chrono::Utc;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;

use crate::templates::components::footer::Footer;
use crate::templates::pages::index::CrateMeta;
use crate::templates::pages::index::IndexPage;
use crate::templates::pages::not_found::NotFoundPage;

mod templates;

/// Copy static assets into documentation output directory.
///
/// Copies all files under crate-local `assets/` (CSS, fonts, favicon) into
/// `<workspace_root>/target/doc/assets` using a recursive `cp`.
///
/// # Arguments
/// - `doc_dir` Existing `<workspace>/target/doc` directory.
///
/// # Returns
/// Ok on success.
///
/// # Errors
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

/// Collect all raw values for a given `key` from TOML manifest text.
///
/// Performs a simple line-by-line scan (no full TOML parsing).
/// Multiple occurrences of the same key are all returned in file order. If the
/// key does not appear, an empty vector is returned.
///
/// This is intentionally naive: it does not handle multi-line values, arrays,
/// tables, or stripping inline comments. Its purpose here is to extract simple
/// scalar values (`name = "foo"`, `description = "..."`).
///
/// # Arguments
/// - `content` Entire TOML file contents.
/// - `key` Exact key to match at a line start (after trimming leading space).
///
/// # Returns
/// A vector of raw value strings with surrounding double quotes removed when
/// they appear directly at both ends; may be empty.
fn get_toml_values(content: &str, key: &str) -> Vec<String> {
    let mut res = vec![];
    for line in content.lines() {
        let trimmed_line = line.trim_start();
        if let Some(rest) = trimmed_line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                res.push(after_eq.trim().trim_matches('"').to_string());
            }
        }
    }
    res
}

/// Generate consolidated styled workspace documentation.
fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    let workspace_root = ytil_system::get_workspace_root()?;
    let doc_dir = workspace_root.join("target/doc");
    // Always remove docs dir if present:
    // - caching problems
    // - implementing manual cache busting
    if let Err(error) = std::fs::remove_dir_all(&doc_dir)
        && !matches!(error.kind(), NotFound)
    {
        bail!("cannot remove docs dir | doc_dir={} error={error}", doc_dir.display());
    }

    // Always (re)generate docs for all workspace crates (including private items) first.
    // Use RUSTDOCFLAGS to enforce warnings-as-errors (portable across cargo versions).
    ytil_cmd::silent_cmd("cargo")
        .current_dir(&workspace_root)
        // Using env because supplying `"--", "-D", "warnings"` doesn't seem to work.
        .env("RUSTDOCFLAGS", "-Dwarnings")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .status()?
        .exit_ok()?;

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

        let names = get_toml_values(&content, "name");
        let desc = get_toml_values(&content, "description")
            .first()
            .cloned()
            .ok_or_else(|| eyre!("missing crate description | cargo_toml={cargo_toml:#?}"))?;

        // Only include crates that actually have a generated index (documentation produced).
        for name in names {
            let index_html = doc_dir.join(&name).join("index.html");
            if index_html.is_file() {
                crates.push(CrateMeta {
                    name,
                    description: desc.clone(),
                });
            }
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
