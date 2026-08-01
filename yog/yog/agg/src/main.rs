use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match Cmd::from_env()? {
        Cmd::Help => print!("{}", include_str!("../help.txt")),
        Cmd::SessionsList => cmds::sessions::list::run()?,
        Cmd::SessionsListJson(args) => cmds::sessions::list::run_json(&args)?,
        Cmd::CodexCompact => cmds::codex::compact::run()?,
    }
    Ok(())
}
