#![feature(exit_status_error)]

use std::path::Path;
use std::path::PathBuf;

use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;

const ZELLIJ_PLUGINS_PATH: &[&str] = &[".config", "zellij", "plugins"];
const ZELLIJ_LAYOUTS_PATH: &[&str] = &[".config", "zellij", "layouts"];
const WASM_TARGET: &str = "wasm32-wasip1";

pub struct PluginInstallSpec {
    pub dir_name: &'static str,
    pub wasm_name: &'static str,
}

/// Build a Zellij WASM plugin and copy it into the local Zellij plugins directory.
///
/// # Errors
/// Returns an error when the WASM target cannot be installed, the plugin build fails, or the artifact cannot be copied.
pub fn build_and_install_plugin(spec: &PluginInstallSpec, is_debug: bool) -> rootcause::Result<()> {
    let wasm_path = build_wasm(spec, is_debug)
        .context("failed to build wasm plugin")
        .attach_with(|| format!("plugin={}", spec.dir_name))?;
    install_wasm_plugin(&wasm_path, spec.wasm_name)
        .context("failed to install wasm plugin")
        .attach_with(|| format!("plugin={}", spec.dir_name))?;
    Ok(())
}

/// Copy a Zellij layout file into the local Zellij layouts directory.
///
/// # Errors
/// Returns an error when the install directory cannot be resolved or created, or the layout file cannot be copied.
pub fn install_layout_file(source: &Path, layout_name: &str) -> rootcause::Result<()> {
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
