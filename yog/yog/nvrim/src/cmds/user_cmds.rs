//! User command registration helpers.
//!
//! Defines a small set of ergonomic commands (copy all, highlight listing, lazy maintenance, select all, fkr generator)
//! and registers them with Neovim while notifying on failures.

use nvim_oxi::api::StringOrFunction;
use nvim_oxi::api::opts::CreateCommandOpts;
use nvim_oxi::api::opts::CreateCommandOptsBuilder;
use nvim_oxi::api::types::CommandArgs;

const USER_CMDS: [(&str, &str, &str); 6] = [
    ("CopyAll", "Copy all", ":%y+"),
    ("Highlights", "FzfHighlights", ":FzfLua highlights"),
    ("LazyProfile", "Lazy profile", ":Lazy profile"),
    ("LazyUpdate", "Lazy update", ":Lazy update"),
    ("SelectAll", "Select all", "normal! ggVG"),
    (
        "Fkr",
        "Gen string with fkr",
        ":lua require('nvrim').fkr.insert_string()",
    ),
];

/// Creates user commands (including `Fkr` for random string generation).
pub fn create() {
    for (cmd_name, desc, cmd) in USER_CMDS {
        create_user_cmd(cmd_name, cmd, &CreateCommandOptsBuilder::default().desc(desc).build());
    }
}

/// Registers a single user command with Neovim.
fn create_user_cmd<Cmd>(name: &str, command: Cmd, opts: &CreateCommandOpts)
where
    Cmd: StringOrFunction<CommandArgs, ()>,
{
    if let Err(error) = nvim_oxi::api::create_user_command(name, command, opts) {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot create user command | name={name} opts={opts:#?} error={error:#?}"
        ));
    }
}
