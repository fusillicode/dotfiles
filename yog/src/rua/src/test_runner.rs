use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use mlua::prelude::*;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;

/// Runs the function enclosing the supplied [`CursorPosition`] as a Rust test in the first Wezterm
/// pane that matches the tab and the current working directory of the pane of the supplied
/// [`CursorPosition`].
///
/// Returns an error in case of:
/// - the file referenced by [`CursorPosition`] is not a Rust file
/// - no enclosing function can be found for the supplied [`CursorPosition`]
/// - any external error related to interacting with Wezterm and the external test runner app
///   (i.e. cargo make)
pub fn run_test(_lua: &Lua, cursor_position: CursorPosition) -> LuaResult<()> {
    let test_name = get_enclosing_fn_name_of_position(
        cursor_position.path.as_path(),
        Point::from(&cursor_position),
    )?
    .ok_or(anyhow::anyhow!(
        "no enclosing fn found for {cursor_position:?}"
    ))?;

    let cur_pane_id = utils::wezterm::get_current_pane_id()
        .map_err(|e| anyhow::anyhow!(e))
        .with_context(|| "cannot get current Wezterm pane id")?;

    let wez_panes = utils::wezterm::get_all_panes()
        .map_err(|e| anyhow::anyhow!(e))
        .with_context(|| "cannot get Wezterm panes")?;

    let cur_pane = wez_panes
        .iter()
        .find(|p| p.pane_id == cur_pane_id)
        .ok_or(anyhow::anyhow!(
            "current pane not found among Wezterm panes {wez_panes:?}"
        ))?;

    let test_runner_pane = wez_panes
        .iter()
        .find(|p| { p.is_sibling_terminal_pane_of(cur_pane) })
        .ok_or(anyhow::anyhow!(
            "cannot find a pane sibling to {cur_pane:#?} among Wezterm panes {wez_panes:#?} where to run the test {test_name}"
        ))?;

    utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                "{} && {} && {}",
                utils::wezterm::send_text_to_pane(
                    &format!("'cargo make test {test_name}'"),
                    test_runner_pane.pane_id
                ),
                utils::wezterm::submit_pane(test_runner_pane.pane_id),
                utils::wezterm::activate_pane(test_runner_pane.pane_id)
            ),
        ])
        .spawn()
        .with_context(|| {
            format!("error executing test {test_name} in Wezterm pane {test_runner_pane:?}")
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    Ok(())
}

/// Represents the position of the cursor inside a terminal editor opened on an existing file
/// inside a Wezterm pane.
#[derive(Debug)]
pub struct CursorPosition {
    pub path: PathBuf,
    pub row: usize,
    pub col: usize,
}

impl From<&CursorPosition> for Point {
    fn from(value: &CursorPosition) -> Self {
        let row = value.row.checked_sub(1).unwrap_or_default();
        let column = value.col.checked_sub(1).unwrap_or_default();
        Self { row, column }
    }
}

impl FromLua for CursorPosition {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::Table(table) = value {
            let out = Self {
                path: PathBuf::from(LuaErrorContext::with_context(
                    table.get::<String>("path"),
                    |_| format!("missing path in LuaTable {table:?}",),
                )?),
                row: LuaErrorContext::with_context(table.get("row"), |_| {
                    format!("missing row in LuaTable {table:?}")
                })?,
                col: LuaErrorContext::with_context(table.get("col"), |_| {
                    format!("missing col in LuaTable {table:?}")
                })?,
            };
            return Ok(out);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "CurrentPosition".into(),
            message: Some(format!("expected a table got {value:?}")),
        })
    }
}

fn get_enclosing_fn_name_of_position(
    file_path: &Path,
    position: Point,
) -> anyhow::Result<Option<String>> {
    if file_path.extension().is_some_and(|ext| ext != "rs") {
        anyhow::bail!("{file_path:?} is not a Rust file");
    }
    let src = std::fs::read(file_path).with_context(|| format!("Error reading {file_path:?}"))?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .with_context(|| "error setting parser language")?;

    let src_tree = parser
        .parse(&src, None)
        .ok_or(anyhow::anyhow!("error parsing src {file_path:?} as Rust"))?;

    let node_at_position = src_tree
        .root_node()
        .descendant_for_point_range(position, position);

    Ok(get_enclosing_fn_name_of_node(&src, node_at_position))
}

fn get_enclosing_fn_name_of_node(src: &[u8], node: Option<Node>) -> Option<String> {
    const FN_NODE_KINDS: &[&str] = &[
        "function",
        "function_declaration",
        "function_definition",
        "function_item",
        "method",
        "method_declaration",
        "method_definition",
        "method_item",
    ];
    let mut current_node = node;
    while let Some(node) = current_node {
        if FN_NODE_KINDS.contains(&node.kind())
            && let Some(fn_node_name) = node
                .child_by_field_name("name")
                .or_else(|| node.child_by_field_name("identifier"))
            && let Ok(fn_name) = fn_node_name.utf8_text(src)
            && !fn_name.is_empty()
        {
            return Some(fn_name.to_string());
        }
        current_node = node.parent();
    }
    None
}
