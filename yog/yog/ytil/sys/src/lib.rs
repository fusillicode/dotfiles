//! Provide cohesive system helpers: args, paths, symlinks, permissions, atomic copy, clipboard.
//!
//! Offer small utilities for CLI tools: joining thread handles, building home-relative paths,
//! manipulating filesystem entries (chmod, symlinks, atomic copy) and clipboard integration.

#![feature(exit_status_error)]

use std::process::Command;
use std::thread::JoinHandle;

use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
pub use pico_args;

pub mod cli;
pub mod dir;
pub mod file;
pub mod lsof;
pub mod rm;

/// Joins a thread handle and returns the result, handling join errors as [`eyre::Error`].
/// Awaits a `JoinHandle` and unwraps the inner `Result`.
///
/// # Errors
/// - The task panicked.
/// - The task returned an error.
pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle
        .join()
        .map_err(|err| eyre!("error joining handle | error={err:#?}"))?
}

/// Opens the given argument using the system's default opener.
///
/// # Arguments
/// - `arg` The argument to open (e.g., URL, or file path).
///
/// # Returns
/// Returns `Ok(())` if the command executes successfully.
///
/// # Errors
/// - The `open` command fails to execute.
/// - The `open` command exits with a non-zero status.
pub fn open(arg: &str) -> color_eyre::Result<()> {
    let cmd = "open";
    Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} {arg}"))
        .status()
        .wrap_err_with(|| eyre!("error running cmd | cmd={cmd:?} arg={arg:?}"))?
        .exit_ok()
        .wrap_err_with(|| eyre!("error cmd exit not ok | cmd={cmd:?} arg={arg:?}"))?;
    Ok(())
}
