use std::path::PathBuf;

use color_eyre::eyre::eyre;
use lightningcss::stylesheet::ParserOptions;
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::stylesheet::StyleSheet;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    println!("cargo:rerun-if-changed=templates/style.css");

    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let css_input_path = manifest_dir.join("templates/style.css");
    let css_code = std::fs::read_to_string(&css_input_path)?;

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let css_output_path = out_dir.join("style.min.css");

    let minified = if std::env::var_os("NO_MINIFY").is_some() {
        css_code
    } else {
        minify_css(&css_code)?
    };

    std::fs::write(&css_output_path, minified)?;
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
