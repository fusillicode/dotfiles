#![feature(exit_status_error)]

use rootcause::report;
use ytil_sys::cli::Args as _;

mod install;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("install") => {
            let is_debug = args.iter().any(|a| a == "--debug");
            install::run(is_debug)
        }
        Some(unknown) => Err(report!("unknown argument").attach(format!("argument={unknown}"))),
        None => Err(report!("missing subcommand").attach("subcommand=install")),
    }
}
