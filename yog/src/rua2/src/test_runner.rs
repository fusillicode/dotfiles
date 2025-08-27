use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use nvim_oxi::Function;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;

pub fn run_test() -> Object {
    Object::from(Function::<(), Option<_>>::from_fn(run_test_core))
}

fn run_test_core(_: ()) -> Option<()> {
    let cur_buf = Buffer::current();
    let cur_win = Window::current();

    let position = cur_win.get_cursor().map_or_else(
        |error| {
            crate::log_error(&format!(
                "fail to get cursor from current window {cur_win:#?}, error {error:#?}"
            ));
            None
        },
        |(row, column)| Some(Point { row, column }),
    )?;
    let file_path = cur_buf.get_name().map_or_else(
        |error| {
            crate::log_error(&format!("fail to get buffer name, error {error:#?}"));
            None
        },
        |x| Some(PathBuf::from(x.to_string_lossy().to_string())),
    )?;

    let Some(test_name) = get_enclosing_fn_name_of_position(&file_path, position)
        .inspect_err(|error| {
            crate::log_error(&format!("fail to get enclosing fn for {position:#?}, error {error:#?}"));
        })
        .ok()?
    else {
        crate::log_error(&format!("no enclosing fn found for {position:#?}"));
        return None;
    };

    let cur_pane_id = utils::wezterm::get_current_pane_id()
        .inspect_err(|error| {
            crate::log_error(&format!("cannot get current Wezterm pane id, error {error:#?}"));
        })
        .ok()?;

    let wez_panes = utils::wezterm::get_all_panes(&[])
        .inspect_err(|error| {
            crate::log_error(&format!("cannot get Wezterm panes, error {error:#?}"));
        })
        .ok()?;

    let Some(cur_pane) = wez_panes.iter().find(|p| p.pane_id == cur_pane_id) else {
        crate::log_error(&format!("current pane not found among Wezterm panes {wez_panes:#?}"));
        return None;
    };

    let Some(test_runner_pane) = wez_panes.iter().find(|p| p.is_sibling_terminal_pane_of(cur_pane)) else {
        crate::log_error(&format!(
            "cannot find a pane sibling to {cur_pane:#?} among Wezterm panes {wez_panes:#?} where to run the test {test_name}"
        ));
        return None;
    };

    let test_runner_app = get_test_runner_app_for_path(&file_path)
        .inspect_err(|error| {
            crate::log_error(&format!("fail to get test runner app, error {error:#?}"));
        })
        .ok()?;

    let test_run_cmd = format!("'{test_runner_app} {test_name}'");
    let send_text_to_pane_cmd = utils::wezterm::send_text_to_pane_cmd(&test_run_cmd, test_runner_pane.pane_id);
    let submit_pane_cmd = utils::wezterm::submit_pane_cmd(test_runner_pane.pane_id);

    utils::cmd::silent_cmd("sh")
        .args(["-c", &format!("{send_text_to_pane_cmd} && {submit_pane_cmd}")])
        .spawn()
        .inspect_err(|error| {
            crate::log_error(&format!(
                "error executing {test_run_cmd:#?} in Wezterm pane {test_runner_pane:#?}, error {error:#?}"
            ));
        })
        .ok()?;

    Some(())
}

fn get_enclosing_fn_name_of_position(file_path: &Path, position: Point) -> anyhow::Result<Option<String>> {
    anyhow::ensure!(
        file_path.extension().is_some_and(|ext| ext == "rs"),
        "{file_path:#?} must be Rust file"
    );
    let src = std::fs::read(file_path).with_context(|| format!("Error reading {file_path:#?}"))?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .with_context(|| "error setting parser language")?;

    let src_tree = parser
        .parse(&src, None)
        .ok_or(anyhow!("error parsing src {file_path:#?} as Rust"))?;

    let node_at_position = src_tree.root_node().descendant_for_point_range(position, position);

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

/// Get the application to use to run the tests based on the presence of a `Makefile.toml`
/// in the root of a git repository where the supplied [Path] resides.
///
/// If the file is found "cargo make test" is used to run the tests.
/// "cargo test" is used otherwise.
///
/// Assumptions:
/// 1. We're always working in a git repository
/// 2. no custom config file for cargo-make
fn get_test_runner_app_for_path(path: &Path) -> anyhow::Result<&'static str> {
    let git_repo_root = utils::git::get_repo_root(Some(path)).map_err(|e| anyhow!(e))?;

    if std::fs::read_dir(git_repo_root)?.any(|res| {
        res.as_ref()
            .map(|de| de.file_name() == "Makefile.toml")
            .unwrap_or(false)
    }) {
        return Ok("cargo make test");
    }

    Ok("cargo test")
}
