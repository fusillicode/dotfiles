//! Launch a Zellij session with a vertical tab sidebar plugin.
//!
//! Subcommands:
//! - `install` — build the WASM plugin, deploy it, and provision Claude/Cursor hooks.
//! - `hook` — unified agent lifecycle hook entry point (used by Claude and Cursor hooks).
//! - `git-stat` — print `path insertions deletions untracked` per path (one line each).
//!
//! # Errors
//! - Zellij invocation fails.
#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use agm_core::Agent;
use agm_core::AgentEventKind;
use agm_core::GitStat;
use owo_colors::OwoColorize as _;
use serde_json::Value;
use ytil_cmd::CmdExt as _;
use ytil_sys::cli::Args;

const SESSION_NAME: &str = "agm";
const LAYOUT_NAME: &str = "agm";

const ZELLIJ_PLUGINS_PATH: &[&str] = &[".config", "zellij", "plugins"];
const WASM_FILENAME: &str = "agm-plugin.wasm";
const INSTALL_NAME: &str = "agm.wasm";

fn session_exists(name: &str) -> bool {
    ytil_zellij::list_sessions().is_ok_and(|sessions| sessions.iter().any(|s| s.name == name))
}

fn build_wasm(is_debug: bool) -> rootcause::Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_dir = manifest_dir.join("plugin");
    let workspace_target = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| rootcause::report!("cannot resolve workspace target from CARGO_MANIFEST_DIR"))?
        .join("target");
    let wasm_target = workspace_target.join("wasm-plugins");

    ytil_cmd::silent_cmd("rustup")
        .args(["target", "add", "wasm32-wasip1"])
        .status()?
        .exit_ok()?;

    let mut cmd = ytil_cmd::silent_cmd("cargo");
    cmd.args(["build", "--target", "wasm32-wasip1"]);
    cmd.current_dir(&plugin_dir);
    cmd.env("CARGO_TARGET_DIR", &wasm_target);
    if !is_debug {
        cmd.arg("--release");
    }
    cmd.status()?.exit_ok()?;

    let profile = if is_debug { "debug" } else { "release" };
    Ok(wasm_target.join("wasm32-wasip1").join(profile).join(WASM_FILENAME))
}

fn install_wasm(built: &Path) -> rootcause::Result<()> {
    let install_dir = ytil_sys::dir::build_home_path(ZELLIJ_PLUGINS_PATH)?;
    std::fs::create_dir_all(&install_dir)?;
    let dest = install_dir.join(INSTALL_NAME);
    ytil_sys::file::atomic_cp(built, &dest)?;
    println!("{} {}", "Installed".green().bold(), dest.display());
    Ok(())
}

fn provision_hooks(agent: Agent) -> rootcause::Result<()> {
    let config = agent.config_path();
    if config.is_empty() {
        return Ok(());
    }

    let Ok(path) = ytil_sys::dir::build_home_path(config) else {
        print_skipped(agent);
        return Ok(());
    };

    let mut doc: Value = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw)?
    } else if path.parent().is_some_and(Path::is_dir) {
        serde_json::from_str(agent.default_config())?
    } else {
        print_skipped(agent);
        return Ok(());
    };

    let hooks = doc
        .as_object_mut()
        .ok_or_else(|| rootcause::report!("{} root is not an object", path.display()))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| rootcause::report!("{} hooks is not an object", path.display()))?;

    for &(event, payload) in agent.hook_events() {
        let cmd = agent.hook_command(payload);
        let event_arr = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| rootcause::report!("hooks.{event} is not an array"))?;

        if let Some(entry) = find_agm_entry(agent, event_arr) {
            entry["command"] = Value::String(cmd);
        } else {
            event_arr.push(new_hook_entry(agent, &cmd));
        }
    }

    let out = serde_json::to_string_pretty(&doc)? + "\n";
    std::fs::write(&path, out)?;
    println!(
        "{} {} hooks in {}",
        "Provisioned".green().bold(),
        agent.name(),
        path.display()
    );
    Ok(())
}

fn print_skipped(agent: Agent) {
    println!(
        "{} {} hooks — config not found",
        "Skipped".yellow().bold(),
        agent.name(),
    );
}

fn find_agm_entry(agent: Agent, arr: &mut [Value]) -> Option<&mut Value> {
    match agent {
        Agent::Claude => arr.iter_mut().find_map(|group| {
            group.get_mut("hooks")?.as_array_mut()?.iter_mut().find(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains("agm hook") || c.contains(agm_core::PIPE_NAME))
            })
        }),
        Agent::Cursor | Agent::Codex => arr.iter_mut().find(|e| {
            e.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|c| c.contains("agm hook"))
        }),
    }
}

fn new_hook_entry(agent: Agent, cmd: &str) -> Value {
    match agent {
        Agent::Claude => serde_json::json!({
            "hooks": [{ "type": "command", "command": cmd }]
        }),
        Agent::Cursor | Agent::Codex => serde_json::json!({ "command": cmd }),
    }
}

fn hook(raw_agent: &str, raw_payload: &str) {
    let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
    println!("{{}}");
    let Ok(pane_id) = std::env::var("ZELLIJ_PANE_ID") else {
        return;
    };
    let (Ok(agent), Ok(kind)) = (Agent::from_name(raw_agent), AgentEventKind::parse(raw_payload)) else {
        eprintln!("agm hook: invalid args agent={raw_agent:?} payload={raw_payload:?}");
        return;
    };
    let _ = std::process::Command::new("zellij")
        .args([
            "pipe",
            "--name",
            agm_core::PIPE_NAME,
            "--args",
            &format!("pane_id={pane_id},agent={}", agent.name()),
            "--",
            kind.as_str(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn git_stat(cwd: &str) -> GitStat {
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return GitStat::default();
    };

    let (insertions, deletions) = repo
        .diff_index_to_workdir(None, None)
        .and_then(|d| d.stats())
        .map_or((0, 0), |s| (s.insertions(), s.deletions()));

    let new_files = repo
        .statuses(Some(
            git2::StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .exclude_submodules(true)
                .include_ignored(false),
        ))
        .map_or(0, |st| {
            st.iter().filter(|s| s.status().contains(git2::Status::WT_NEW)).count()
        });

    let is_worktree = repo.is_worktree();

    GitStat {
        insertions,
        deletions,
        new_files,
        is_worktree,
    }
}

fn install(is_debug: bool) -> rootcause::Result<()> {
    let built = build_wasm(is_debug)?;
    install_wasm(&built)?;
    provision_hooks(Agent::Claude)?;
    provision_hooks(Agent::Cursor)?;
    Ok(())
}

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
            return install(is_debug);
        }
        Some("hook") => {
            let agent = args.get(1).map_or("", String::as_str);
            let payload = args.get(2).map_or("", String::as_str);
            hook(agent, payload);
            return Ok(());
        }
        Some("git-stat") => {
            let paths = args.get(1..);
            for cwd in paths.into_iter().flatten() {
                let stat = git_stat(cwd);
                println!("{cwd} {stat}");
            }
            return Ok(());
        }
        _ => {}
    }

    let session = args.first().map_or(SESSION_NAME, String::as_str);

    if !session_exists(session) {
        agm_core::clean_state_dir(session);
        if ytil_zellij::is_active() {
            ytil_cmd::silent_cmd("zellij")
                .args(["--new-session-with-layout", LAYOUT_NAME, "--session", session])
                .exec()?;
        } else {
            return ytil_zellij::new_session_with_layout(session, LAYOUT_NAME);
        }
    }

    ytil_zellij::attach_session(session)
}
