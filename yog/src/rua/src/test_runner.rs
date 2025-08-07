use mlua::prelude::*;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;
// use tree_sitter_rust;

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

    let Some(fn_node) = get_enclosing_fn_node(parsed_src.root_node(), Point::from(editor_position))
    else {
        return Ok("No enclosing fn node found".into());
    };

    let Ok(test_name) = fn_node.utf8_text(&src) else {
        return Ok("Cannot get fn node name".into());
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
        Self {
            row: value.row,
            column: value.col,
        }
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

fn get_enclosing_fn_node(root: Node, position: Point) -> Option<Node> {
    const FN_NODE_KINDS: &[&str] = &[
        "function",
        "function_declaration",
        "function_definition",
        "function_item",
        "method",
        "method_declaration",
        "method_definition",
    ];
    let mut node = root.named_descendant_for_point_range(position, position)?;
    loop {
        if FN_NODE_KINDS.contains(&node.kind()) {
            return node.child_by_field_name("name");
        }
        if let Some(parent) = node.parent() {
            node = parent;
            continue;
        }
        return None;
    }
}
