use std::path::Path;
use std::path::PathBuf;

use agg::AGENTS_PIPE;
use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt;
use serde_json::Map;
use serde_json::Value;
use ytil_agents::agent::Agent;
use ytil_agents::agent::AgentEventKind;

const ZELLIJ_PLUGINS_PATH: &[&str] = &[".config", "zellij", "plugins"];
const ZELLIJ_LAYOUTS_PATH: &[&str] = &[".config", "zellij", "layouts"];
const WASM_TARGET: &str = "wasm32-wasip1";
const AGG_PLUGIN: PluginInstallSpec = PluginInstallSpec {
    dir_name: "agg",
    wasm_name: "agg.wasm",
};
const ZCP_PLUGIN: PluginInstallSpec = PluginInstallSpec {
    dir_name: "zcp",
    wasm_name: "zcp.wasm",
};
const ZOP_PLUGIN: PluginInstallSpec = PluginInstallSpec {
    dir_name: "zop",
    wasm_name: "zop.wasm",
};
const LAYOUT_FILENAME: &str = "agg.kdl";
const GEMINI_HOOK_NAME_PREFIX: &str = "agg-gemini-";
const OPENCODE_AGG_PLUGIN_PATH: &[&str] = &[".config", "opencode", "plugins", "agg.ts"];
const OPENCODE_TEMPLATE_FILENAME: &str = "agg.ts.template";
const OPENCODE_PIPE_PLACEHOLDER: &str = "{{AGENTS_PIPE}}";

struct PluginInstallSpec {
    dir_name: &'static str,
    wasm_name: &'static str,
}

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    install_agg_plugin_and_hooks(is_debug)?;
    build_and_install_plugin(&ZCP_PLUGIN, is_debug).context("failed to install zcp wasm plugin")?;
    build_and_install_plugin(&ZOP_PLUGIN, is_debug).context("failed to install zop wasm plugin")?;
    Ok(())
}

fn install_agg_plugin_and_hooks(is_debug: bool) -> rootcause::Result<()> {
    build_and_install_plugin(&AGG_PLUGIN, is_debug).context("failed to install agg wasm plugin")?;
    install_layout().context("failed to install zellij layout")?;
    install_hooks(Agent::Claude).context("failed to install Claude hooks")?;
    install_hooks(Agent::Cursor).context("failed to install Cursor hooks")?;
    install_hooks(Agent::Codex).context("failed to install Codex hooks")?;
    install_hooks(Agent::Gemini).context("failed to install Gemini hooks")?;
    install_opencode_plugin().context("failed to install Opencode hooks")?;
    ensure_nudge_icons_dir().context("failed to create nudge icons directory")?;
    Ok(())
}

fn build_and_install_plugin(spec: &PluginInstallSpec, is_debug: bool) -> rootcause::Result<()> {
    let wasm_path = build_wasm(spec, is_debug)
        .context("failed to build wasm plugin")
        .attach_with(|| format!("plugin={}", spec.dir_name))?;
    install_wasm_plugin(&wasm_path, spec.wasm_name)
        .context("failed to install wasm plugin")
        .attach_with(|| format!("plugin={}", spec.dir_name))?;
    Ok(())
}

fn build_wasm(spec: &PluginInstallSpec, is_debug: bool) -> rootcause::Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_dir = manifest_dir
        .parent()
        .ok_or_else(|| rootcause::report!("cannot resolve zj dir from CARGO_MANIFEST_DIR"))?
        .join(spec.dir_name);
    let workspace_target = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| rootcause::report!("cannot resolve workspace target from CARGO_MANIFEST_DIR"))?
        .join("target");

    build_wasm_plugin(&plugin_dir, &workspace_target, spec.wasm_name, is_debug)
}

fn install_layout() -> rootcause::Result<()> {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("zellij")
        .join(LAYOUT_FILENAME);
    install_layout_file(&source, LAYOUT_FILENAME)
}

fn build_wasm_plugin(
    plugin_dir: &Path,
    workspace_target_root: &Path,
    artifact_name: &str,
    is_debug: bool,
) -> rootcause::Result<PathBuf> {
    ytil_cmd::silent_cmd("rustup")
        .args(["target", "add", WASM_TARGET])
        .status()
        .context("failed to spawn rustup command")
        .attach_with(|| format!("target={WASM_TARGET}"))?
        .exit_ok()
        .context("failed to add wasm32-wasip1 target")
        .attach_with(|| format!("target={WASM_TARGET}"))?;

    let wasm_target = workspace_target_root.join("wasm-plugins");
    let mut cmd = ytil_cmd::silent_cmd("cargo");
    cmd.args(["build", "--target", WASM_TARGET]);
    cmd.current_dir(plugin_dir);
    cmd.env("CARGO_TARGET_DIR", &wasm_target);
    if !is_debug {
        cmd.arg("--release");
    }
    cmd.status()
        .context("failed to spawn cargo build command")
        .attach_with(|| format!("target={WASM_TARGET}"))?
        .exit_ok()
        .context("failed to build wasm plugin")
        .attach_with(|| format!("target={WASM_TARGET}"))
        .attach_with(|| format!("plugin_dir={}", plugin_dir.display()))?;

    let profile = if is_debug { "debug" } else { "release" };
    Ok(wasm_target.join(WASM_TARGET).join(profile).join(artifact_name))
}

fn install_wasm_plugin(built: &Path, install_name: &str) -> rootcause::Result<()> {
    let install_dir = ytil_sys::dir::build_home_path(ZELLIJ_PLUGINS_PATH)
        .context("failed to determine zellij plugins directory")
        .attach_with(|| format!("plugins_path={ZELLIJ_PLUGINS_PATH:?}"))?;

    std::fs::create_dir_all(&install_dir)
        .context("failed to create install directory")
        .attach_with(|| format!("install_dir={}", install_dir.display()))?;

    let dest = install_dir.join(install_name);
    ytil_sys::file::atomic_cp(built, &dest)
        .context("failed to copy wasm plugin to install location")
        .attach_with(|| format!("from={}", built.display()))
        .attach_with(|| format!("to={}", dest.display()))?;

    println!("{} {} to {}", "Copied".green().bold(), built.display(), dest.display());
    Ok(())
}

fn install_layout_file(source: &Path, layout_name: &str) -> rootcause::Result<()> {
    let install_dir = ytil_sys::dir::build_home_path(ZELLIJ_LAYOUTS_PATH)
        .context("failed to determine zellij layouts directory")
        .attach_with(|| format!("layouts_path={ZELLIJ_LAYOUTS_PATH:?}"))?;

    std::fs::create_dir_all(&install_dir)
        .context("failed to create zellij layouts directory")
        .attach_with(|| format!("install_dir={}", install_dir.display()))?;

    let dest = install_dir.join(layout_name);
    ytil_sys::file::atomic_cp(source, &dest)
        .context("failed to copy layout to install location")
        .attach_with(|| format!("from={}", source.display()))
        .attach_with(|| format!("to={}", dest.display()))?;

    println!("{} {} to {}", "Copied".green().bold(), source.display(), dest.display());
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

    remove_all_agg_entries(agent, hooks);

    for &(event, payload) in agent.hook_events() {
        let cmd = hook_command(agent, payload);
        let event_arr = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| rootcause::report!("hooks.{event} is not an array"))?;

        remove_agg_entries(agent, event_arr);
        event_arr.push(new_hook_entry(agent, event, &cmd));
    }

    let out =
        serde_json::to_string_pretty(&doc).context(format!("failed to serialize config for {}", agent.name()))? + "\n";
    std::fs::write(&path, out)
        .context("failed to write config file")
        .attach_with(|| format!("path={}", path.display()))
        .attach_with(|| format!("agent={}", agent.name()))?;

    println!("{} hooks in {}", "Patched".green().bold(), path.display());

    Ok(())
}

fn install_opencode_plugin() -> rootcause::Result<()> {
    let Ok(path) = ytil_sys::dir::build_home_path(OPENCODE_AGG_PLUGIN_PATH).attach("agent=opencode") else {
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
        .join(OPENCODE_TEMPLATE_FILENAME);
    let plugin = std::fs::read_to_string(&template)
        .context("failed to read opencode plugin template")
        .attach_with(|| format!("from={}", template.display()))
        .map(|template| template.replace(OPENCODE_PIPE_PLACEHOLDER, AGENTS_PIPE))?;
    std::fs::write(&path, plugin)
        .context("failed to write opencode plugin file")
        .attach_with(|| format!("path={}", path.display()))?;

    println!(
        "{} {} to {}",
        "Copied".green().bold(),
        template.display(),
        path.display()
    );
    Ok(())
}

fn ensure_nudge_icons_dir() -> rootcause::Result<()> {
    let home_dir =
        ytil_sys::dir::build_home_path(&[] as &[&str]).context("failed to determine nudge icon cache directory")?;
    let icon_dir = home_dir.join(".cache").join("zj").join("agg").join("nude-icons");
    std::fs::create_dir_all(&icon_dir)
        .context("failed to create nudge icon cache directory")
        .attach_with(|| format!("path={}", icon_dir.display()))?;
    println!("{} local nudge icons in {}", "Using".green().bold(), icon_dir.display());
    Ok(())
}

fn print_skipped(agent: Agent) {
    println!(
        "{} {} hooks — config not found",
        "Skipped".yellow().bold(),
        agent.name(),
    );
}

fn hook_command(agent: Agent, kind: AgentEventKind) -> String {
    let pipe = format!(
        "zellij pipe --name {AGENTS_PIPE} --args \"pane_id=$ZELLIJ_PANE_ID,agent={}\" -- {} >/dev/null 2>&1 || true",
        agent.name(),
        kind.as_str()
    );
    let echo = if matches!(agent, Agent::Gemini) {
        "; echo '{}'"
    } else {
        ""
    };
    format!("cat >/dev/null 2>&1 || true; {pipe}{echo}")
}

fn hook_name(agent: Agent, event: &str) -> Option<&'static str> {
    match agent {
        Agent::Gemini => match event {
            "SessionStart" => Some("agg-gemini-session-start"),
            "BeforeAgent" => Some("agg-gemini-before-agent"),
            "BeforeModel" => Some("agg-gemini-before-model"),
            "BeforeToolSelection" => Some("agg-gemini-before-tool-selection"),
            "BeforeTool" => Some("agg-gemini-before-tool"),
            "Notification" => Some("agg-gemini-notification"),
            "AfterAgent" => Some("agg-gemini-after-agent"),
            "SessionEnd" => Some("agg-gemini-session-end"),
            _ => None,
        },
        Agent::Claude | Agent::Codex | Agent::Cursor | Agent::Opencode => None,
    }
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

    disabled.retain(|entry| !is_agg_gemini_disabled_entry(entry));

    Ok(())
}

fn is_agg_gemini_disabled_entry(entry: &Value) -> bool {
    entry.as_str().is_some_and(|entry| {
        entry.starts_with(GEMINI_HOOK_NAME_PREFIX) || (entry.contains(AGENTS_PIPE) && entry.contains("agent=gemini"))
    })
}

fn is_agg_hook_command(command: &str) -> bool {
    command.contains(AGENTS_PIPE)
}

fn is_agg_gemini_hook(hook: &Value) -> bool {
    hook.get("name")
        .and_then(|name| name.as_str())
        .is_some_and(|name| name.starts_with(GEMINI_HOOK_NAME_PREFIX))
        || hook
            .get("command")
            .and_then(|command| command.as_str())
            .is_some_and(is_agg_hook_command)
}

fn remove_agg_entries(agent: Agent, arr: &mut Vec<Value>) {
    match agent {
        Agent::Claude | Agent::Codex => arr.retain(|group| {
            !group
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| {
                    hooks.iter().any(|hook| {
                        hook.get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(is_agg_hook_command)
                    })
                })
        }),
        Agent::Gemini => arr.retain(|group| {
            !group
                .get("hooks")
                .and_then(|hooks| hooks.as_array())
                .is_some_and(|hooks| hooks.iter().any(is_agg_gemini_hook))
        }),
        Agent::Cursor => arr.retain(|entry| {
            !entry
                .get("command")
                .and_then(|c| c.as_str())
                .is_some_and(is_agg_hook_command)
        }),
        Agent::Opencode => {}
    }
}

fn remove_all_agg_entries(agent: Agent, hooks: &mut serde_json::Map<String, Value>) {
    let empty_events: Vec<String> = hooks
        .iter_mut()
        .filter_map(|(event, value)| {
            let arr = value.as_array_mut()?;
            remove_agg_entries(agent, arr);
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
            if let Some(name) = hook_name(agent, event) {
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
    use ytil_agents::agent::AgentEventKind;

    use super::*;

    #[test]
    fn test_remove_all_agg_entries_removes_stale_codex_events() {
        let mut hooks = serde_json::json!({
            "PreToolUse": [
                new_hook_entry(Agent::Codex, "PreToolUse", &hook_command(Agent::Codex, AgentEventKind::Busy)),
                {
                    "hooks": [{ "type": "command", "command": "echo keep-me" }]
                }
            ],
            "PostToolUse": [new_hook_entry(
                Agent::Codex,
                "PostToolUse",
                &hook_command(Agent::Codex, AgentEventKind::Busy)
            )],
            "PermissionRequest": [new_hook_entry(
                Agent::Codex,
                "PermissionRequest",
                &hook_command(Agent::Codex, AgentEventKind::Busy)
            )],
            "SessionEnd": [new_hook_entry(Agent::Codex, "SessionEnd", &hook_command(Agent::Codex, AgentEventKind::Exit))],
            "UserPromptSubmit": [new_hook_entry(
                Agent::Codex,
                "UserPromptSubmit",
                &hook_command(Agent::Codex, AgentEventKind::Busy)
            )]
        });

        remove_all_agg_entries(Agent::Codex, hooks.as_object_mut().unwrap());

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
    fn test_ensure_gemini_hooks_enabled_unblocks_agg_without_touching_unrelated_entries() {
        let mut root = serde_json::json!({
            "hooksConfig": {
                "enabled": false,
                "disabled": [
                    "agg-gemini-before-tool",
                    "custom-hook",
                    "cat >/dev/null 2>&1 || true; zellij pipe --name agg-agent --args \"pane_id=$ZELLIJ_PANE_ID,agent=gemini\" -- busy >/dev/null 2>&1 || true; echo '{}'"
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
    fn test_remove_all_agg_entries_removes_stale_gemini_events_by_name_and_command() {
        let mut hooks = serde_json::json!({
            "BeforeTool": [
                new_hook_entry(Agent::Gemini, "BeforeTool", &hook_command(Agent::Gemini, AgentEventKind::Busy)),
                {
                    "hooks": [{
                        "type": "command",
                        "command": "cat >/dev/null 2>&1 || true; zellij pipe --name agg-agent --args \"pane_id=$ZELLIJ_PANE_ID,agent=gemini\" -- busy >/dev/null 2>&1 || true; echo '{}'"
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

        remove_all_agg_entries(Agent::Gemini, hooks.as_object_mut().unwrap());

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
            &hook_command(Agent::Gemini, AgentEventKind::Busy),
        );

        let expected = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": hook_command(Agent::Gemini, AgentEventKind::Busy),
                "name": "agg-gemini-before-tool-selection"
            }]
        });

        assert_eq!(actual, expected);
    }
}
