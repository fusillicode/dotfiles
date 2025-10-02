use std::path::PathBuf;

use color_eyre::eyre::eyre;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::stylesheet::StyleSheet;

const ASSETS_DIR: &str = "assets";

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    println!("cargo:rerun-if-changed=assets/style.css");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let css_raw_path = manifest_dir.join(ASSETS_DIR).join("style.css");
    let css_raw = std::fs::read_to_string(&css_raw_path)?;

    let css_minified = if std::env::var_os("NO_MINIFY").is_some() {
        css_raw
    } else {
        minify_css(&css_raw)?
    };

    let css_minified_path = manifest_dir.join(ASSETS_DIR).join("style.min.css");

    std::fs::write(&css_minified_path, css_minified)?;
    Ok(())
}

fn minify_css(css_code: &str) -> color_eyre::Result<String> {
    let sheet = StyleSheet::parse(
        css_code,
        ParserOptions {
            filename: "style.css".into(),
            error_recovery: true,
            ..Default::default()
        },
    )
    .map_err(|error| eyre!(format!("CSS parse error={error:?}")))?;

    Ok(sheet
        .to_css(PrinterOptions {
            minify: true,
            ..Default::default()
        })
        .map_err(|error| eyre!(format!("CSS print error={error:?}")))?
        .code)
}
