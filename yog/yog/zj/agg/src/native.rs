use ytil_sys::cli::Args as _;

mod git_stat;
mod install;
mod nudge;

pub fn run() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.is_empty() || args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("install") => {
            return install::run(args.iter().any(|arg| arg == "--debug"));
        }
        Some("git-stat") => {
            for cwd in args.get(1..).into_iter().flatten() {
                let stat = git_stat::run(cwd);
                println!("{cwd} {stat}");
            }
            return Ok(());
        }
        Some("nudge") => {
            let (name, body, image_path) = match args.as_slice() {
                [_, name, body] => (name, body, None),
                [_, name, body, image_path] => (name, body, Some(image_path.as_str())),
                _ => rootcause::bail!("usage: agg nudge <name> <body> [image-path]"),
            };
            return nudge::run(name, body, image_path);
        }
        _ => {}
    }
    rootcause::bail!("usage: agg install [--debug] | agg git-stat <paths...> | agg nudge <name> <body> [image-path]")
}
