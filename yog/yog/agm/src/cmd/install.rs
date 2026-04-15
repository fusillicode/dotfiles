use std::path::Path;
use std::path::PathBuf;

use agm_core::agent::AGENTS_PIPE;
use agm_core::agent::Agent;
use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt;
use serde_json::Map;
use serde_json::Value;

const ZELLIJ_PLUGINS_PATH: &[&str] = &[".config", "zellij", "plugins"];
const ZELLIJ_LAYOUTS_PATH: &[&str] = &[".config", "zellij", "layouts"];
const WASM_FILENAME: &str = "agm-plugin.wasm";
const INSTALL_NAME: &str = "agm.wasm";
const LAYOUT_FILENAME: &str = "agm.kdl";
const GEMINI_HOOK_NAME_PREFIX: &str = "agm-gemini-";

pub fn install_plugin_and_hooks(is_debug: bool) -> rootcause::Result<()> {
    let wasm_path = build_wasm(is_debug).context("failed to build wasm plugin")?;
    install_wasm(&wasm_path).context("failed to install wasm plugin")?;
    install_layout().context("failed to install zellij layout")?;
    install_hooks(Agent::Claude).context("failed to install Claude hooks")?;
    install_hooks(Agent::Cursor).context("failed to install Cursor hooks")?;
    install_hooks(Agent::Codex).context("failed to install Codex hooks")?;
    install_hooks(Agent::Gemini).context("failed to install Gemini hooks")?;
    install_opencode_plugin().context("failed to install Opencode hooks")?;
    Ok(())
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

    let target = "wasm32-wasip1";

    ytil_cmd::silent_cmd("rustup")
        .args(["target", "add", target])
        .status()
        .context("failed to spawn rustup command")
        .attach_with(|| format!("target={target}"))?
        .exit_ok()
        .context("failed to add wasm32-wasip1 target")
        .attach_with(|| format!("target={target}"))?;

    let mut cmd = ytil_cmd::silent_cmd("cargo");
    cmd.args(["build", "--target", target]);
    cmd.current_dir(&plugin_dir);
    cmd.env("CARGO_TARGET_DIR", &wasm_target);
    if !is_debug {
        cmd.arg("--release");
    }
    cmd.status()
        .context("failed to spawn cargo build command")
        .attach_with(|| format!("target={target}"))?
        .exit_ok()
        .context("failed to build wasm plugin")
        .attach_with(|| format!("target={target}"))
        .attach_with(|| format!("plugin_dir={}", plugin_dir.display()))?;

    let profile = if is_debug { "debug" } else { "release" };
    Ok(wasm_target.join("wasm32-wasip1").join(profile).join(WASM_FILENAME))
}

fn install_wasm(built: &Path) -> rootcause::Result<()> {
    let install_dir = ytil_sys::dir::build_home_path(ZELLIJ_PLUGINS_PATH)
        .context("failed to determine zellij plugins directory")
        .attach_with(|| format!("plugins_path={ZELLIJ_PLUGINS_PATH:?}"))?;

    std::fs::create_dir_all(&install_dir)
        .context("failed to create install directory")
        .attach_with(|| format!("install_dir={}", install_dir.display()))?;

    let dest = install_dir.join(INSTALL_NAME);
    ytil_sys::file::atomic_cp(built, &dest)
        .context("failed to copy wasm plugin to install location")
        .attach_with(|| format!("from={}", built.display()))
        .attach_with(|| format!("to={}", dest.display()))?;

    println!("{} {}", "Installed".green().bold(), dest.display());
    Ok(())
}

fn install_layout() -> rootcause::Result<()> {
    let install_dir = ytil_sys::dir::build_home_path(ZELLIJ_LAYOUTS_PATH)
        .context("failed to determine zellij layouts directory")
        .attach_with(|| format!("layouts_path={ZELLIJ_LAYOUTS_PATH:?}"))?;

    std::fs::create_dir_all(&install_dir)
        .context("failed to create zellij layouts directory")
        .attach_with(|| format!("install_dir={}", install_dir.display()))?;

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("zellij")
        .join(LAYOUT_FILENAME);
    let dest = install_dir.join(LAYOUT_FILENAME);
    ytil_sys::file::atomic_cp(&source, &dest)
        .context("failed to copy agm layout to install location")
        .attach_with(|| format!("from={}", source.display()))
        .attach_with(|| format!("to={}", dest.display()))?;

    println!("{} {}", "Installed".green().bold(), dest.display());
    Ok(())
}

/// Load the JSON hook config at `path`, or create it from [`Agent::default_config`] when missing.
fn read_hooks_json_or_default(path: &Path, agent: Agent) -> rootcause::Result<Value> {
    if path.exists() {
        let raw = std::fs::read_to_string(path).context("failed to read config file")?;
        let doc: Value = serde_json::from_str(&raw).context("failed to parse config file")?;
        return Ok(doc);
    }

    let Some(parent) = path.parent() else {
        return Err(rootcause::report!(
            "hook config path has no parent directory: {}",
            path.display()
        ));
    };

    std::fs::create_dir_all(parent).context("failed to create agent config directory")?;

    let doc: Value = serde_json::from_str(agent.default_config()).context("failed to parse default config")?;

    Ok(doc)
}

fn install_hooks(agent: Agent) -> rootcause::Result<()> {
    let config = agent.config_path();
    if config.is_empty() {
        print_skipped(agent);
        return Ok(());
    }

    let Ok(path) = ytil_sys::dir::build_home_path(config).attach_with(|| format!("agent={}", agent.name())) else {
        print_skipped(agent);
        return Ok(());
    };

    let mut doc = read_hooks_json_or_default(&path, agent)
        .attach_with(|| format!("path={}", path.display()))
        .attach_with(|| format!("agent={}", agent.name()))?;

    let root = doc
        .as_object_mut()
        .ok_or_else(|| rootcause::report!("{} root is not an object", path.display()))?;

    if matches!(agent, Agent::Gemini) {
        ensure_gemini_hooks_enabled(root)?;
    }

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| rootcause::report!("{} hooks is not an object", path.display()))?;

    remove_all_agm_entries(agent, hooks);

    for &(event, payload) in agent.hook_events() {
        let cmd = agent.hook_command(payload);
        let event_arr = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| rootcause::report!("hooks.{event} is not an array"))?;

        remove_agm_entries(agent, event_arr);
        event_arr.push(new_hook_entry(agent, event, &cmd));
    }

    let out =
        serde_json::to_string_pretty(&doc).context(format!("failed to serialize config for {}", agent.name()))? + "\n";
    std::fs::write(&path, out)
        .context("failed to write config file")
        .attach_with(|| format!("path={}", path.display()))
        .attach_with(|| format!("agent={}", agent.name()))?;

    println!(
        "{} {} hooks in {}",
        "Installed".green().bold(),
        agent.name(),
        path.display()
    );

    Ok(())
}

fn install_opencode_plugin() -> rootcause::Result<()> {
    let config_path = Agent::Opencode.config_path();
    let Ok(path) = ytil_sys::dir::build_home_path(config_path).attach("agent=opencode") else {
        print_skipped(Agent::Opencode);
        return Ok(());
    };

    let Some(dir) = path.parent() else {
        print_skipped(Agent::Opencode);
        return Ok(());
    };

    std::fs::create_dir_all(dir)
        .context("failed to create opencode plugins directory")
        .attach_with(|| format!("dir={}", dir.display()))?;

    let template = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("opencode")
        .join("agm.ts");
    ytil_sys::file::atomic_cp(&template, &path)
        .context("failed to copy opencode plugin file")
        .attach_with(|| format!("from={}", template.display()))
        .attach_with(|| format!("path={}", path.display()))?;

    println!("{} opencode plugin in {}", "Installed".green().bold(), path.display());
    Ok(())
}

fn print_skipped(agent: Agent) {
    println!(
        "{} {} hooks — config not found",
        "Skipped".yellow().bold(),
        agent.name(),
    );
}

fn ensure_gemini_hooks_enabled(root: &mut Map<String, Value>) -> rootcause::Result<()> {
    let hooks_config = root
        .entry("hooksConfig")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| rootcause::report!("hooksConfig is not an object"))?;

    hooks_config.insert("enabled".to_string(), Value::Bool(true));

    let disabled = hooks_config
        .entry("disabled")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| rootcause::report!("hooksConfig.disabled is not an array"))?;

    disabled.retain(|entry| !is_agm_gemini_disabled_entry(entry));

    Ok(())
}

fn is_agm_gemini_disabled_entry(entry: &Value) -> bool {
    entry.as_str().is_some_and(|entry| {
        entry.starts_with(GEMINI_HOOK_NAME_PREFIX) || (entry.contains(AGENTS_PIPE) && entry.contains("agent=gemini"))
    })
}

fn is_agm_hook_command(command: &str) -> bool {
    command.contains(AGENTS_PIPE)
}

fn is_agm_gemini_hook(hook: &Value) -> bool {
    hook.get("name")
        .and_then(|name| name.as_str())
        .is_some_and(|name| name.starts_with(GEMINI_HOOK_NAME_PREFIX))
        || hook
            .get("command")
            .and_then(|command| command.as_str())
            .is_some_and(is_agm_hook_command)
}

fn remove_agm_entries(agent: Agent, arr: &mut Vec<Value>) {
    match agent {
        Agent::Claude | Agent::Codex => arr.retain(|group| {
            !group
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook.get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(is_agm_hook_command)
                    })
                })
        }),
        Agent::Gemini => arr.retain(|group| {
            !group
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| hooks.iter().any(is_agm_gemini_hook))
        }),
        Agent::Cursor => arr.retain(|entry| {
            !entry
                .get("command")
                .and_then(|c| c.as_str())
                .is_some_and(is_agm_hook_command)
        }),
        Agent::Opencode => {}
    }
}

fn remove_all_agm_entries(agent: Agent, hooks: &mut serde_json::Map<String, Value>) {
    let empty_events: Vec<String> = hooks
        .iter_mut()
        .filter_map(|(event, value)| {
            let arr = value.as_array_mut()?;
            remove_agm_entries(agent, arr);
            arr.is_empty().then(|| event.clone())
        })
        .collect();

    for event in empty_events {
        hooks.remove(&event);
    }
}

fn new_hook_entry(agent: Agent, event: &str, cmd: &str) -> Value {
    match agent {
        Agent::Claude | Agent::Codex => serde_json::json!({
            "hooks": [{ "type": "command", "command": cmd }]
        }),
        Agent::Gemini => {
            let mut hook = Map::from_iter([
                ("type".to_string(), Value::String("command".to_string())),
                ("command".to_string(), Value::String(cmd.to_string())),
            ]);
            if let Some(name) = agent.hook_name(event) {
                hook.insert("name".to_string(), Value::String(name.to_string()));
            }
            serde_json::json!({ "hooks": [Value::Object(hook)] })
        }
        Agent::Cursor => serde_json::json!({ "command": cmd }),
        Agent::Opencode => serde_json::json!({}),
    }
}

#[cfg(test)]
mod tests {
    use agm_core::agent::AgentEventKind;

    use super::*;

    #[test]
    fn test_remove_all_agm_entries_removes_stale_codex_events() {
        let mut hooks = serde_json::json!({
            "PreToolUse": [
                new_hook_entry(Agent::Codex, "PreToolUse", &Agent::Codex.hook_command(AgentEventKind::Busy)),
                {
                    "hooks": [{ "type": "command", "command": "echo keep-me" }]
                }
            ],
            "SessionEnd": [new_hook_entry(Agent::Codex, "SessionEnd", &Agent::Codex.hook_command(AgentEventKind::Exit))],
            "UserPromptSubmit": [new_hook_entry(
                Agent::Codex,
                "UserPromptSubmit",
                &Agent::Codex.hook_command(AgentEventKind::Busy)
            )]
        });

        remove_all_agm_entries(Agent::Codex, hooks.as_object_mut().unwrap());

        let expected = serde_json::json!({
            "PreToolUse": [
                {
                    "hooks": [{ "type": "command", "command": "echo keep-me" }]
                }
            ]
        });

        assert_eq!(hooks, expected);
    }

    #[test]
    fn test_ensure_gemini_hooks_enabled_unblocks_agm_without_touching_unrelated_entries() {
        let mut root = serde_json::json!({
            "hooksConfig": {
                "enabled": false,
                "disabled": [
                    "agm-gemini-before-tool",
                    "custom-hook",
                    "cat >/dev/null 2>&1 || true; zellij pipe --name agm-agent --args \"pane_id=$ZELLIJ_PANE_ID,agent=gemini\" -- busy >/dev/null 2>&1 || true; echo '{}'"
                ]
            }
        });

        ensure_gemini_hooks_enabled(root.as_object_mut().unwrap()).unwrap();

        let expected = serde_json::json!({
            "hooksConfig": {
                "enabled": true,
                "disabled": ["custom-hook"]
            }
        });

        assert_eq!(root, expected);
    }

    #[test]
    fn test_remove_all_agm_entries_removes_stale_gemini_events_by_name_and_command() {
        let mut hooks = serde_json::json!({
            "BeforeTool": [
                new_hook_entry(Agent::Gemini, "BeforeTool", &Agent::Gemini.hook_command(AgentEventKind::Busy)),
                {
                    "hooks": [{
                        "type": "command",
                        "command": "cat >/dev/null 2>&1 || true; zellij pipe --name agm-agent --args \"pane_id=$ZELLIJ_PANE_ID,agent=gemini\" -- busy >/dev/null 2>&1 || true; echo '{}'"
                    }]
                },
                {
                    "hooks": [{
                        "type": "command",
                        "command": "echo keep-me",
                        "name": "custom-hook"
                    }]
                }
            ]
        });

        remove_all_agm_entries(Agent::Gemini, hooks.as_object_mut().unwrap());

        let expected = serde_json::json!({
            "BeforeTool": [
                {
                    "hooks": [{
                        "type": "command",
                        "command": "echo keep-me",
                        "name": "custom-hook"
                    }]
                }
            ]
        });

        assert_eq!(hooks, expected);
    }

    #[test]
    fn test_new_hook_entry_gemini_uses_stable_name() {
        let actual = new_hook_entry(
            Agent::Gemini,
            "BeforeToolSelection",
            &Agent::Gemini.hook_command(AgentEventKind::Busy),
        );

        let expected = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": Agent::Gemini.hook_command(AgentEventKind::Busy),
                "name": "agm-gemini-before-tool-selection"
            }]
        });

        assert_eq!(actual, expected);
    }
}
