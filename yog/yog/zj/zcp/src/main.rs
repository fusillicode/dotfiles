#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(any(test, target_arch = "wasm32"))]
#[cfg_attr(test, expect(dead_code, reason = "plugin entrypoint is constructed by Zellij"))]
mod wasm;
#[cfg(target_arch = "wasm32")]
use zellij_tile::prelude::*;

#[cfg(target_arch = "wasm32")]
register_plugin!(wasm::plugin::State);

#[cfg(not(target_arch = "wasm32"))]
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    native::run()
}
