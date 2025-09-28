use std::path::Path;
use std::path::PathBuf;

use askama::Template;
use chrono::Utc;
use color_eyre::eyre::OptionExt;
use color_eyre::eyre::bail;

// Embed minified CSS produced at build time so runtime does not depend on OUT_DIR.
const MINIFIED_STYLE_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/style.min.css"));

fn main() -> color_eyre::eyre::Result<()> {
    color_eyre::install()?;

    let doc_dir = get_existing_doc_dir()?;

    let mut crates: Vec<String> = std::fs::read_dir(&doc_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| Path::new(&doc_dir).join(name).join("index.html").is_file())
        .collect();
    crates.sort();

    let css_dest_path = doc_dir.join("style.css");
    std::fs::write(&css_dest_path, MINIFIED_STYLE_CSS)?;

    let index = Index {
        title: "Yog Workspace Documentation",
        heading: "Yog Workspace Crates",
        crates: &crates,
        generated: Utc::now().to_rfc3339(),
        repo_url: "https://github.com/fusillicode/dotfiles",
    };
    let html = index.render()?;

    let index_path = doc_dir.join("index.html");
    std::fs::create_dir_all(doc_dir)?;
    std::fs::write(&index_path, html)?;

    Ok(())
}

#[derive(Template)]
#[template(path = "index.html")]
struct Index<'a> {
    title: &'a str,
    heading: &'a str,
    crates: &'a [String],
    generated: String,
    repo_url: &'a str,
}

fn get_existing_doc_dir() -> color_eyre::Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let workspace_root = manifest_dir
        .parent() // .../yog/yog
        .and_then(|p| p.parent()) // .../yog
        .ok_or_eyre(format!("cannot get workspace root from manifest_dir={}", manifest_dir.display()))?;

    let doc_dir = workspace_root.join("target/doc");
    if !doc_dir.exists() {
        bail!(
            "documentation directory '{}' does not exist; run 'cargo doc --workspace' first",
            doc_dir.display()
        )
    }
    Ok(doc_dir)
}
