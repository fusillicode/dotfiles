use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

mod auto_cmds;
mod user_cmds;

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
