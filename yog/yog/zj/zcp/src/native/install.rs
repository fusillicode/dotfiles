use rootcause::prelude::ResultExt;

const ZCP_PLUGIN: zj::PluginInstallSpec = zj::PluginInstallSpec {
    dir_name: "zcp",
    wasm_name: "zcp.wasm",
};

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    zj::build_and_install_plugin(&ZCP_PLUGIN, is_debug).context("failed to install zcp wasm plugin")?;
    Ok(())
}
