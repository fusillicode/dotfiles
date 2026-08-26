use std::path::PathBuf;

use rootcause::report;

use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| report!("HOME environment variable is not set"))?;

    match Cmd::from_env()? {
        Cmd::Help => print!("{}", include_str!("../help.txt")),
        Cmd::SessionsList => cmds::sessions::list::run(&home_dir)?,
        Cmd::SessionsListJson(args) => cmds::sessions::list::run_json(&args, &home_dir)?,
        Cmd::CodexCompact => cmds::codex::compact::run()?,
    }
    Ok(())
}
