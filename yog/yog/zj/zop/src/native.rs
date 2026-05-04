use ytil_sys::cli::Args as _;

mod install;

pub fn run() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.is_empty() || args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "install") {
        return install::run(args.iter().any(|arg| arg == "--debug"));
    }
    rootcause::bail!("usage: zop install [--debug]")
}
