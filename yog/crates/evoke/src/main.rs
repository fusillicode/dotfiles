//! Format, lint, build, and deploy workspace binaries and Nvim libs.
//!
//! # Errors
//! - Cargo commands or file copy operations fail.
#![feature(exit_status_error)]

mod cargo_metadata;
mod cmds;

/// Format, lint, build, and deploy workspace binaries and Nvim libs.
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match cmds::Cmd::from_env()? {
        cmds::Cmd::Help => println!(include_str!("../help.txt")),
        cmds::Cmd::Ci(command) => command.run(&ytil_sys::dir::get_workspace_root()?)?,
        cmds::Cmd::Local(mut args) => cmds::local::run(&mut args)?,
    }
    Ok(())
}
