use std::path::Path;
use std::process::Command;

use rootcause::prelude::ResultExt;

const PLUGINS: &[&str] = &["agg", "zcp", "znt", "zop"];

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    for plugin in PLUGINS {
        run_plugin_install(plugin, is_debug)?;
    }
    Ok(())
}

fn run_plugin_install(plugin: &str, is_debug: bool) -> rootcause::Result<()> {
    let executable = plugin_executable(plugin)?;
    let mut cmd = Command::new(&executable);
    cmd.arg("install");
    if is_debug {
        cmd.arg("--debug");
    }
    cmd.status()
        .context("failed to spawn plugin install command")
        .attach_with(|| format!("plugin={plugin}"))
        .attach_with(|| format!("executable={}", executable.display()))?
        .exit_ok()
        .context("plugin install command failed")
        .attach_with(|| format!("plugin={plugin}"))
        .attach_with(|| format!("executable={}", executable.display()))?;
    Ok(())
}

fn plugin_executable(plugin: &str) -> rootcause::Result<std::path::PathBuf> {
    let current_exe = std::env::current_exe().context("failed to resolve current executable")?;
    let Some(parent) = current_exe.parent() else {
        return Ok(Path::new(plugin).to_path_buf());
    };
    let sibling = parent.join(plugin);
    if sibling.exists() {
        return Ok(sibling);
    }
    Ok(Path::new(plugin).to_path_buf())
}
