use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match Cmd::from_env()? {
        Cmd::Help(help) => print!("{}", help.text()),
        Cmd::Sessions => cmds::sessions::run()?,
        Cmd::Start {
            session,
            external_layout,
        } => cmds::start::run(&session, external_layout)?,
    }
    Ok(())
}
