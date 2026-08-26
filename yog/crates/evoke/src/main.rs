//! Format, lint, build, and deploy workspace binaries and Nvim libs.
//!
//! # Errors
//! - Cargo commands or file copy operations fail.
#![feature(exit_status_error)]

use ytil_sys::cli::Args;

mod cargo_metadata;
mod ci;
mod local;

/// Format, lint, build, and deploy workspace binaries and Nvim libs.
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let mut args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    if let Some(command) = ci::cmd_from_args(&args)? {
        return command.run(&ytil_sys::dir::get_workspace_root()?);
    }

    local::run(&mut args)
}
