//! Local Rust repo maintenance commands.

use ytil_sys::pico_args::Arguments;

use crate::cmds::Cmd;

mod cmds;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    match Cmd::from_env()? {
        Cmd::Help(help) => print!("{}", help.text()),
        Cmd::Repo(args) => crate::cmds::repo::run(Arguments::from_vec(args))?,
        Cmd::Rsl(args) => {
            let output = crate::cmds::rsl::run(Arguments::from_vec(args))?;
            if output.is_empty() {
                return Ok(());
            }
            println!("{}", output.render()?);
            std::process::exit(1);
        }
    }
    Ok(())
}
