use std::path::Path;
use std::process::Output;

use ytil_cmd::CmdError;
use ytil_cmd::CmdExt;

/// Runs `cargo doc` with strict warning enforcement across the workspace.
///
/// Executes `cargo doc --all --no-deps --document-private-items` in the specified workspace root,
/// setting `RUSTDOCFLAGS=-Dwarnings` to treat documentation warnings as errors.
///
/// # Errors
/// - If the `cargo doc` command fails or exits with a non-zero status.
/// - If documentation warnings are present (due to `-Dwarnings`).
pub fn generate_rust_doc(workspace_root: &Path) -> Result<Output, Box<CmdError>> {
    ytil_cmd::silent_cmd("cargo")
        .current_dir(workspace_root.display().to_string())
        // Using env because supplying `"--", "-D", "warnings"` doesn't seem to work.
        .env("RUSTDOCFLAGS", "-Dwarnings")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .exec()
        .map_err(Box::new)
}
