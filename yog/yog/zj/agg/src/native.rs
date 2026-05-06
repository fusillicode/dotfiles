use ytil_sys::cli::Args;

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
            let (summary, body, tab_id, pane_id, image_path) = match args.as_slice() {
                [_, summary, body, tab_id, pane_id] => (summary, body, tab_id, pane_id, None),
                [_, summary, body, tab_id, pane_id, image_path] => {
                    (summary, body, tab_id, pane_id, Some(image_path.as_str()))
                }
                _ => rootcause::bail!("usage: agg nudge <summary> <body> <tab-id> <pane-id> [image-path]"),
            };
            let tab_id = tab_id.parse()?;
            let pane_id = pane_id.parse()?;
            return nudge::run(nudge::RunInput {
                summary,
                body,
                tab_id,
                pane_id,
                image_path,
            });
        }
        _ => {}
    }
    rootcause::bail!(
        "usage: agg install [--debug] | agg git-stat <paths...> | agg nudge <summary> <body> <tab-id> <pane-id> [image-path]"
    )
}
