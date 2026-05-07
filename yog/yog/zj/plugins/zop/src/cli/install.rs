use rootcause::prelude::ResultExt;

const ZOP_PLUGIN: zj::PluginInstallSpec = zj::PluginInstallSpec {
    dir_name: "zop",
    wasm_name: "zop.wasm",
};

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    zj::build_and_install_plugin(&ZOP_PLUGIN, is_debug).context("failed to install zop wasm plugin")?;
    Ok(())
}
