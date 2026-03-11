use nvim_oxi::Dictionary;
use ytil_zellij::Direction;

use crate::buffer::token_under_cursor;
use crate::buffer::token_under_cursor::TokenUnderCursor;

pub fn dict() -> Dictionary {
    dict! {
        "open_token_under_cursor": fn_from!(open_token_under_cursor),
        "copy_enclosing_function": fn_from!(copy_enclosing_function),
        "reveal_in_finder": fn_from!(reveal_in_finder),
    }
}

fn copy_enclosing_function(_: ()) -> Option<()> {
    let file_path = ytil_noxi::buffer::get_absolute_path(Some(&nvim_oxi::api::get_current_buf()))?;
    let enclosing_fn = ytil_noxi::tree_sitter::get_enclosing_fn_name_of_position(&file_path)?;
    ytil_sys::file::cp_to_system_clipboard(&mut enclosing_fn.as_bytes())
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!(
                "error copying content to system clipboard | content={enclosing_fn:?} error={err:#?}"
            ));
        })
        .ok()?;
    nvim_oxi::print!("Enclosing fn name copied to clipboard: {enclosing_fn}");
    Some(())
}

fn open_token_under_cursor(_: ()) -> Option<()> {
    let token_under_cursor = token_under_cursor::get(())?;

    match token_under_cursor {
        TokenUnderCursor::BinaryFile(_) | TokenUnderCursor::MaybeTextFile { .. } => None,
        TokenUnderCursor::TextFile { path, lnum, col } => {
            let open = if ytil_zellij::is_active() {
                open_in_zellij_pane
            } else {
                open_in_nvim_split
            };
            open(&path, lnum, col)
        }
        TokenUnderCursor::Url(arg) | TokenUnderCursor::Directory(arg) => ytil_sys::open(&arg)
            .inspect_err(|err| {
                ytil_noxi::notify::error(format!("{err:?}"));
            })
            .ok(),
    }
}

fn open_in_zellij_pane(path: &str, lnum: Option<i64>, col: Option<i64>) -> Option<()> {
    let cursor_cmd = format!("call cursor({}, {})", lnum.unwrap_or_default(), col.unwrap_or_default());

    let result = ytil_zellij::pane_count().and_then(|pane_count| {
        if pane_count > 1 {
            ytil_zellij::move_focus(Direction::Right)?;
            let is_nvim =
                ytil_zellij::focused_pane_command()?.is_some_and(|cmd| cmd.contains("nvim") || cmd.contains("vim"));
            if is_nvim {
                ytil_zellij::write_byte(0x1b)?;
                ytil_zellij::write_chars(&format!(":edit {path} | {cursor_cmd}\r"))?;
            } else {
                let cursor_arg = format!("+'{cursor_cmd}'");
                ytil_zellij::write_chars(&format!("nvim {cursor_arg} {path}\r"))?;
            }
            return Ok(());
        }
        let cursor_arg = format!("+'{cursor_cmd}'");
        ytil_zellij::new_pane(Direction::Right, &["nvim", &cursor_arg, path])?;
        ytil_zellij::resize_increase(Direction::Left, 3)?;
        Ok(())
    });

    result
        .inspect_err(|err| ytil_noxi::notify::error(format!("{err:#?}")))
        .ok()
}

fn open_in_nvim_split(path: &str, lnum: Option<i64>, col: Option<i64>) -> Option<()> {
    let open_path_cmd = format!(
        "edit {path} | call cursor({}, {})",
        lnum.unwrap_or_default(),
        col.unwrap_or_default()
    );

    let vim_script = if let Some(win_num) =
        ytil_noxi::window::find_with_buffer("").and_then(|(win, _)| ytil_noxi::window::get_number(&win))
    {
        format!("{win_num} wincmd w | {open_path_cmd}")
    } else {
        let width = crate::layout::compute_width(70)?;
        format!("vsplit | vertical resize {width} | {open_path_cmd}")
    };

    ytil_noxi::common::exec_vim_script(&vim_script, None);
    Some(())
}

fn reveal_in_finder(_: ()) -> Option<()> {
    let file_path = ytil_noxi::buffer::get_absolute_path(Some(&nvim_oxi::api::get_current_buf()))?;
    let Some(parent) = file_path.parent() else {
        ytil_noxi::notify::error(format!(
            "error no parent for current buffer file path | file_path={}",
            file_path.display()
        ));
        return None;
    };
    let Some(parent_str) = parent.to_str() else {
        ytil_noxi::notify::error(format!(
            "error parent path is not valid UTF-8 | path={}",
            parent.display()
        ));
        return None;
    };
    ytil_sys::open(parent_str)
        .inspect_err(|err| {
            ytil_noxi::notify::error(format!("error opening path | path={} error={err:#?}", parent.display()));
        })
        .ok()?;
    Some(())
}
