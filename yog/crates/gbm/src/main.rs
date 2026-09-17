//! Prepare or execute a current Git branch rename.

use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match Cmd::from_env()? {
        Cmd::Help(help) => print!("{}", help.text()),
        Cmd::Pick => cmds::pick::run()?,
        Cmd::Install => cmds::install::run()?,
        Cmd::InitZsh => cmds::init::run(),
        Cmd::Rename(branch_name) => cmds::rename::run(&branch_name)?,
    }
    Ok(())
}
