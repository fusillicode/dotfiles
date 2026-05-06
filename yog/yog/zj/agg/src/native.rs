use ytil_sys::cli::Args;

mod git_stat;
mod install;
mod nudge;

const NUDGE_USAGE: &str = "usage: agg nudge <summary> <body> <tab-id> <pane-id> [image-path] [--session <session>]";
const USAGE: &str = "usage: agg install [--debug] | agg git-stat <paths...> | agg nudge <summary> <body> <tab-id> <pane-id> [image-path] [--session <session>]";

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
            let args = NudgeArgs::try_from(args.get(1..).unwrap_or_default())?;
            let tab_id = args.tab_id.parse()?;
            let pane_id = args.pane_id.parse()?;
            return nudge::run(nudge::NudgeInput {
                summary: args.summary,
                body: args.body,
                tab_id,
                pane_id,
                image_path: args.image_path,
                zj_session: args.zj_session,
            });
        }
        _ => {}
    }
    rootcause::bail!(USAGE)
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct NudgeArgs<'a> {
    summary: &'a str,
    body: &'a str,
    tab_id: &'a str,
    pane_id: &'a str,
    image_path: Option<&'a str>,
    zj_session: Option<&'a str>,
}

impl<'a> TryFrom<&'a [String]> for NudgeArgs<'a> {
    type Error = rootcause::Report;

    fn try_from(args: &'a [String]) -> Result<Self, Self::Error> {
        let mut zj_session = None;
        let mut positional = Vec::new();
        let mut args = args.iter().map(String::as_str);
        while let Some(arg) = args.next() {
            if arg == "--session" {
                let Some(value) = args.next() else {
                    rootcause::bail!(NUDGE_USAGE);
                };
                zj_session = (!value.is_empty()).then_some(value);
            } else {
                positional.push(arg);
            }
        }

        let (summary, body, tab_id, pane_id, image_path) = match positional.as_slice() {
            [summary, body, tab_id, pane_id] => (*summary, *body, *tab_id, *pane_id, None),
            [summary, body, tab_id, pane_id, image_path] => (*summary, *body, *tab_id, *pane_id, Some(*image_path)),
            _ => rootcause::bail!(NUDGE_USAGE),
        };

        Ok(Self {
            summary,
            body,
            tab_id,
            pane_id,
            image_path,
            zj_session,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nudge_args_try_from_with_image_path_and_zj_session_returns_args() {
        let args = vec![
            "Codex done".to_string(),
            "/repo".to_string(),
            "7".to_string(),
            "42".to_string(),
            "/icon.png".to_string(),
            "--session".to_string(),
            "work".to_string(),
        ];

        let parsed = NudgeArgs::try_from(args.as_slice()).unwrap();

        pretty_assertions::assert_eq!(
            parsed,
            NudgeArgs {
                summary: "Codex done",
                body: "/repo",
                tab_id: "7",
                pane_id: "42",
                image_path: Some("/icon.png"),
                zj_session: Some("work"),
            }
        );
    }

    #[test]
    fn test_nudge_args_try_from_without_zj_session_returns_args() {
        let args = vec![
            "Codex done".to_string(),
            "/repo".to_string(),
            "7".to_string(),
            "42".to_string(),
        ];

        let parsed = NudgeArgs::try_from(args.as_slice()).unwrap();

        pretty_assertions::assert_eq!(
            parsed,
            NudgeArgs {
                summary: "Codex done",
                body: "/repo",
                tab_id: "7",
                pane_id: "42",
                image_path: None,
                zj_session: None,
            }
        );
    }
}
