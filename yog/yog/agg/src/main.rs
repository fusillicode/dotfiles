//! Launch a Zellij session with a vertical tab sidebar plugin.
//!
//! Subcommands:
//! - `install` — build the WASM plugin, deploy it, and install Claude, Cursor, Codex, Gemini, and Opencode hooks.
//! - `git-stat` — print git statistics for a directory.
//! - `git-stat` — print `path insertions deletions untracked` per path (one line each).
//!
//! # Errors
//! - Zellij invocation fails.
#![feature(exit_status_error)]

use rootcause::report;
use ytil_cmd::CmdExt as _;
use ytil_sys::cli::Args;

mod cmd;

const SESSION_NAME: &str = "agg";
const LAYOUT_NAME: &str = "agg";

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
            cmd::install::install_plugin_and_hooks(is_debug)
        }
        Some("git-stat") => {
            let paths = args.get(1..);
            for cwd in paths.into_iter().flatten() {
                let stat = cmd::git_stat::run(cwd);
                println!("{cwd} {stat}");
            }
            Ok(())
        }
        Some("sessions") => cmd::sessions::run(),
        None => launch_session(&args),
        Some(unknown) => Err(report!("unknown argument").attach(format!("argument={unknown}"))),
    }
}

fn launch_session(args: &[String]) -> rootcause::Result<()> {
    let session_name = args.first().map_or(SESSION_NAME, String::as_str);

    if ytil_zellij::list_sessions().is_ok_and(|sessions| sessions.iter().any(|s| s.name == session_name)) {
        ytil_zellij::attach_session(session_name)?;
        return Ok(());
    }

    if ytil_zellij::is_active() {
        ytil_cmd::silent_cmd("zellij")
            .args(["--new-session-with-layout", LAYOUT_NAME, "--session", session_name])
            .exec()?;
        return Ok(());
    }

    ytil_zellij::new_session_with_layout(session_name, LAYOUT_NAME)
}
