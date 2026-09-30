//! Retained terminal history, dump format, and external viewer.

/// Terminal scrollback config.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackConfig {
    /// Dump format used by the server-owned scrollback viewer keybinding.
    pub dump_style: ScrollbackDumpStyle,
    /// External editor used by the scrollback viewer.
    pub editor: ScrollbackEditorConfig,
    /// Number of rows retained for each server-side terminal scrollback source.
    pub rows: usize,
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self {
            dump_style: ScrollbackDumpStyle::PlainText,
            editor: ScrollbackEditorConfig {
                program: "nvim",
                // Scrollback opens in a read-only, no-swap nvim profile at the bottom of the dump; Esc quits the
                // temporary viewer so it behaves like a muxr mode instead of a normal editor session.
                args: &[
                    "-u",
                    "~/.config/nvim/minimal.lua",
                    "-R",
                    "-n",
                    "+",
                    "-c",
                    "nnoremap <buffer> <silent> <Esc> :quit!<CR>",
                ],
            },
            rows: 50_000,
        }
    }
}

/// Dump format used by the server-owned scrollback viewer keybinding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollbackDumpStyle {
    PlainText,
    Ansi,
}

/// External editor used by the scrollback viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScrollbackEditorConfig {
    pub program: &'static str,
    /// Args passed before the generated scrollback dump path.
    ///
    /// Args starting with `~/` are expanded against `$HOME` before spawning the editor.
    pub args: &'static [&'static str],
}
