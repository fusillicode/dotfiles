#![feature(exit_status_error)]

use ytil_sys::cli::Args;

mod sessions;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    sessions::run()
}
