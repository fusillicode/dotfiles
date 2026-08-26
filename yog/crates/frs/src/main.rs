//! Local Rust repo maintenance commands.

use rootcause::report;
use ytil_sys::pico_args::Arguments;

mod repo;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let mut cli_args = Arguments::from_env();
    let command = cli_args.subcommand()?;
    match command.as_deref() {
        None => {
            if cli_args.contains("--help") || cli_args.finish().is_empty() {
                print!("{}", include_str!("../help.txt"));
            } else {
                return Err(report!("unsupported frs command"));
            }
        }
        Some("repo") => repo::run(cli_args)?,
        Some(command) => return Err(report!("unsupported frs command").attach(format!("command={command}"))),
    }
    Ok(())
}
