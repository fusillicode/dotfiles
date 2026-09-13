use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let command = match Cmd::from_env() {
        Ok(command) => command,
        Err(error) => {
            print!(include_str!("../help.txt"));
            return Err(error);
        }
    };

    match command {
        Cmd::Help => print!(include_str!("../help.txt")),
        Cmd::Sessions => cmds::sessions::run()?,
        Cmd::Start {
            session,
            external_layout,
        } => cmds::start::run(&session, external_layout)?,
    }
    Ok(())
}
