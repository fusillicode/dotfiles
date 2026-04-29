use std::path::Path;
use std::path::PathBuf;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;

const ZELLIJ_PLUGINS_PATH: &[&str] = &[".config", "zellij", "plugins"];
const WASM_FILENAME: &str = "zcp-plugin.wasm";
const INSTALL_NAME: &str = "zcp.wasm";

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    let wasm_path = build_wasm(is_debug).context("failed to build wasm plugin")?;
    install_wasm(&wasm_path).context("failed to install wasm plugin")?;
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
