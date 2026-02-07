//! Open files (optionally at line:col) in existing Nvim / Helix pane.
//!
//! # Errors
//! - Argument parsing, pane discovery, or command execution fails.
#![feature(exit_status_error)]

use core::str::FromStr;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use ytil_editor::Editor;
use ytil_editor::FileToOpen;
use ytil_sys::cli::Args;

/// Wrapper for environment variables.
struct Env {
    /// The name of the environment variable (static string).
    name: &'static str,
    /// The value of the environment variable (dynamically constructed string).
    value: String,
}

impl Env {
    /// Returns environment variable as tuple.
    pub fn by_ref(&self) -> (&'static str, &str) {
        (self.name, &self.value)
    }
}

/// Creates enriched PATH for `WezTerm` integration.
///
/// # Errors
/// - A required environment variable is missing or invalid Unicode.
fn get_enriched_path_env() -> rootcause::Result<Env> {
    let enriched_path = [
        &std::env::var("PATH").unwrap_or_else(|_| String::new()),
        "/opt/homebrew/bin",
        &ytil_sys::dir::build_home_path(&[".local", "bin"])?.to_string_lossy(),
    ]
    .join(":");

    Ok(Env {
        name: "PATH",
        value: enriched_path,
    })
}

/// Escape single quotes for safe embedding in shell single-quoted strings.
///
/// Replaces each `'` with `'\''` (end quote, escaped quote, begin quote).
fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Open files (optionally at line:col) in existing Nvim / Helix pane.
fn main() -> rootcause::Result<()> {
    let enriched_path_env = get_enriched_path_env()?;
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let Some(editor) = args.first().map(|x| Editor::from_str(x)).transpose()? else {
        return Err(report!("missing editor arg")).attach_with(|| format!("args={args:#?}"));
    };

    let Some(file_to_open) = args.get(1) else {
        return Err(report!("missing file arg")).attach_with(|| format!("args={args:#?}"));
    };

    let pane_id = match args.get(2) {
        Some(x) => x.parse()?,
        None => ytil_wezterm::get_current_pane_id()?,
    };

    let panes = ytil_wezterm::get_all_panes(&[enriched_path_env.by_ref()])?;

    let file_to_open = FileToOpen::try_from((file_to_open.as_str(), pane_id, panes.as_slice()))?;

    let editor_pane_id =
        ytil_wezterm::get_sibling_pane_with_titles(&panes, pane_id, editor.pane_titles()).map(|x| x.pane_id)?;

    let open_file_cmd = editor.open_file_cmd(&file_to_open);
    let escaped_open_file_cmd = escape_single_quotes(&open_file_cmd);

    ytil_cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {} && {}",
                // `wezterm cli send-text $'\e'` sends the "ESC" to `WezTerm` to exit from insert mode
                // https://github.com/wez/wezterm/discussions/3945
                ytil_wezterm::send_text_to_pane_cmd(r"$'\e'", editor_pane_id),
                ytil_wezterm::send_text_to_pane_cmd(&format!("'{escaped_open_file_cmd}'"), editor_pane_id),
                ytil_wezterm::submit_pane_cmd(editor_pane_id),
                ytil_wezterm::activate_pane_cmd(editor_pane_id),
            ),
        ])
        .envs(std::iter::once(enriched_path_env.by_ref()))
        .spawn()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    #[case::no_quotes("hello world", "hello world")]
    #[case::single_quote("it's here", "it'\\''s here")]
    #[case::multiple_quotes("a'b'c", "a'\\''b'\\''c")]
    #[case::only_quote("'", "'\\''")]
    #[case::empty("", "")]
    fn escape_single_quotes_produces_expected_output(#[case] input: &str, #[case] expected: &str) {
        pretty_assertions::assert_eq!(escape_single_quotes(input), expected);
    }
}
