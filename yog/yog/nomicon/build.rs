//! Build script for the `nomicon` binary.
//!
//! - Reads the unminified CSS asset at `assets/style.css`.
//! - Writes the result to `assets/style.min.css`.
//!
//! # Rationale
//!
//! Performing CSS minification in `build.rs` avoids adding a runtime dependency or a
//! separate Node / npm pipeline.

use std::path::PathBuf;

use color_eyre::eyre::eyre;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::stylesheet::StyleSheet;

const ASSETS_DIR: &str = "assets";

/// Minifies raw CSS source into an optimized string.
///
/// - Parses the provided CSS with permissive error recovery.
/// - Serializes it with minification enabled.
///
/// # Arguments
/// - `css_code` Raw (unminified) CSS source as UTF-8 text.
///
/// # Errors
/// - Parse failure: Returned if [`StyleSheet::parse`] cannot recover from invalid CSS.
/// - Print failure: Returned if [`StyleSheet::to_css`] encounters an internal error while serializing.
///
/// # Performance
/// One parse + one print. For small stylesheets this is negligible. If you add more stylesheets consider batching them
/// into a single concatenated parse to reduce overhead.
fn minify_css(css_code: &str) -> color_eyre::Result<String> {
    let sheet = StyleSheet::parse(
        css_code,
        ParserOptions {
            filename: "style.css".into(),
            error_recovery: true,
            ..Default::default()
        },
    )
    .map_err(|error| eyre!(format!("error parsing CSS | error={error:#?}")))?;

    Ok(sheet
        .to_css(PrinterOptions {
            minify: true,
            ..Default::default()
        })
        .map_err(|error| eyre!(format!("error printing CSS | error={error:#?}")))?
        .code)
}

fn main() -> color_eyre::Result<()> {
    println!("cargo:rerun-if-changed=assets/style.css");

    color_eyre::install()?;

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let css_raw_path = manifest_dir.join(ASSETS_DIR).join("style.css");
    let css_raw = std::fs::read_to_string(&css_raw_path)?;

    let css_minified_path = manifest_dir.join(ASSETS_DIR).join("style.min.css");
    let css_minified = minify_css(&css_raw)?;

    std::fs::write(&css_minified_path, css_minified)?;
    Ok(())
}
