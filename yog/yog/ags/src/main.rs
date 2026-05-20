use ytil_sys::cli::Args;

mod sessions;

#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();

    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let args = args.as_slice();
    if args.first().map(String::as_str) == Some("list") && args.get(1).map(String::as_str) == Some("--json") {
        return sessions::list_json(args.get(2..).unwrap_or_default());
    }

    sessions::run()
}
