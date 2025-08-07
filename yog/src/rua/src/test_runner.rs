use mlua::prelude::*;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;

pub fn run(_lua: &Lua, editor_position: EditorPosition) -> LuaResult<String> {
    let Ok(src) = std::fs::read(&editor_position.file_path) else {
        return Ok(format!(
            "Failed to read path of editor position {editor_position:?}"
        ));
    };

    let mut parser = Parser::new();
    let Ok(_) = parser.set_language(&tree_sitter_rust::LANGUAGE.into()) else {
        return Ok("Failed to set parser language".into());
    };

    let Some(parsed_src) = parser.parse(&src, None) else {
        return Ok("Failed to parse code".into());
    };

    let Some(test_name) =
        get_enclosing_fn_node_name(parsed_src.root_node(), &src, Point::from(editor_position))
    else {
        return Ok("No enclosing fn node found".into());
    };

    let Ok(nvim_pane_id) = utils::wezterm::get_current_pane_id() else {
        return Ok("Cannot get current pane id".into());
    };
    let Ok(all_panes) = utils::wezterm::get_all_panes() else {
        return Ok("Cannot get all Wezterm panes".into());
    };
    let Some(nvim_pane) = all_panes.iter().find(|p| p.pane_id == nvim_pane_id) else {
        return Ok("No neovim pane found".into());
    };
    let Some(test_runner_pane) = all_panes.iter().find(|p| {
        p.pane_id != nvim_pane.pane_id && p.tab_id == nvim_pane.tab_id && p.cwd == nvim_pane.cwd
    }) else {
        return Ok(format!(
            "No test runner pane found, nvim_pane {nvim_pane:#?}, all panes {all_panes:#?}"
        ));
    };

    let Ok(_) = utils::cmd::silent_cmd("sh")
        .args([
            "-c",
            &format!(
                r#"
                    wezterm cli send-text 'cargo make test {test_name}' --pane-id '{0}' --no-paste && \
                        printf "\r" | wezterm cli send-text --pane-id '{0}' --no-paste &&
                        wezterm cli activate-pane --pane-id '{0}'
                "#,
                test_runner_pane.pane_id
            ),
        ]).spawn() else {
        return Ok("Error interacting with Wezterm".into());
    };

    Ok(format!("{test_name}, {}", test_runner_pane.pane_id))
}

#[derive(Debug)]
pub struct EditorPosition {
    pub file_path: String,
    pub row: usize,
    pub col: usize,
}

impl From<EditorPosition> for Point {
    fn from(value: EditorPosition) -> Self {
        let row = value.row.checked_sub(1).unwrap_or_default();
        let column = value.col.checked_sub(1).unwrap_or_default();
        Self { row, column }
    }
}

impl FromLua for EditorPosition {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        if let LuaValue::Table(table) = value {
            let out = Self {
                file_path: table.get("path").unwrap(),
                row: table.get("row").unwrap(),
                col: table.get("col").unwrap(),
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

fn get_enclosing_fn_node_name(root: Node, src: &[u8], position: Point) -> Option<String> {
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
    let mut node = root.descendant_for_point_range(position, position);
    while let Some(current_node) = node {
        if FN_NODE_KINDS.contains(&current_node.kind())
            && let Some(fn_node_name) = current_node
                .child_by_field_name("name")
                .or_else(|| current_node.child_by_field_name("identifier"))
            && let Ok(fn_name) = fn_node_name.utf8_text(src)
            && !fn_name.is_empty()
        {
            return Some(fn_name.to_string());
        }
        node = current_node.parent();
    }
    None
}
