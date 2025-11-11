//! User / auto command creation orchestration.
//!
//! Provides a dictionary exposing `create` which defines all autocommands and user commands by
//! delegating to internal modules (`auto_cmds`, `user_cmds`).

use nvim_oxi::Dictionary;

mod auto_cmds;
mod user_cmds;

pub use auto_cmds::create_autocmd;

/// [`Dictionary`] of user command helpers.
pub fn dict() -> Dictionary {
    dict! {
        "create": fn_from!(create),
    }
}

/// Creates all configured autocommands and user commands.
fn create(_: ()) {
    auto_cmds::create();
    user_cmds::create();
}
