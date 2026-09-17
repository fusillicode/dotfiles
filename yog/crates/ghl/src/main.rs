use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match Cmd::from_env()? {
        Cmd::Help(help) => print!("{}", help.text()),
        Cmd::Issue => {
            ytil_gh::log_into_github()?;
            cmds::issue::run()?;
        }
        Cmd::Pr => {
            ytil_gh::log_into_github()?;
            cmds::pr::run()?;
        }
        Cmd::Branch => {
            ytil_gh::log_into_github()?;
            cmds::branch::run()?;
        }
        Cmd::List(args) => cmds::list::run(args)?,
    }
    Ok(())
}
