use rootcause::prelude::ResultExt;

const ZNT_PLUGIN: zj::PluginInstallSpec = zj::PluginInstallSpec {
    dir_name: "znt",
    wasm_name: "znt.wasm",
};

pub fn run(is_debug: bool) -> rootcause::Result<()> {
    zj::build_and_install_plugin(&ZNT_PLUGIN, is_debug).context("failed to install znt wasm plugin")?;
    Ok(())
}
