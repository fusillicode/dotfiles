#![feature(exit_status_error)]

use std::path::Path;
use std::process::Output;

use ytil_cmd::CmdError;
use ytil_cmd::CmdExt;

/// Runs `cargo doc` with strict warning enforcement across the workspace.
///
/// Executes `cargo doc --all --no-deps --document-private-items` in the specified workspace root,
/// setting `RUSTDOCFLAGS=-Dwarnings` to treat documentation warnings as errors.
///
/// # Arguments
/// - `workspace_root` The path to the workspace root directory.
///
/// # Returns
/// `Ok(())` if documentation generation succeeds without warnings.
///
/// # Errors
/// - If the `cargo doc` command fails or exits with a non-zero status.
/// - If documentation warnings are present (due to `-Dwarnings`).
// FIXME: I don't want this...
#[allow(clippy::result_large_err)]
pub fn generate_rust_doc(workspace_root: &Path) -> Result<Output, CmdError> {
    ytil_cmd::silent_cmd("cargo")
        .current_dir(workspace_root.display().to_string())
        // Using env because supplying `"--", "-D", "warnings"` doesn't seem to work.
        .env("RUSTDOCFLAGS", "-Dwarnings")
        .args(["doc", "--all", "--no-deps", "--document-private-items"])
        .exec()
}
