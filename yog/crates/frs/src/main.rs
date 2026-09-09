//! Local Rust repo maintenance commands.

use rootcause::report;
use ytil_sys::pico_args::Arguments;

mod repo;
mod rsl;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let mut cli_args = Arguments::from_env();
    let command = cli_args.subcommand()?;
    match command.as_deref() {
        None => {
            if cli_args.contains("--help") || cli_args.finish().is_empty() {
                print!(include_str!("../help.txt"));
            } else {
                return Err(report!("unsupported frs command"));
            }
        }
        Some("repo") => crate::repo::run(cli_args)?,
        Some("rsl") => {
            let violations = crate::rsl::run(cli_args)?;
            if violations.is_empty() {
                return Ok(());
            }
            println!("{}", serde_json::to_string(&violations)?);
            std::process::exit(1);
        }
        Some(command) => return Err(report!("unsupported frs command").attach(format!("command={command}"))),
    }
    Ok(())
}
