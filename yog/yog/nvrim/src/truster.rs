use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::Window;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;

use crate::dict;
use crate::fn_from;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "run_test": fn_from!(run_test),
    }
}

/// Runs the test function at the current cursor position in a `WezTerm` pane.
fn run_test(_: ()) {
    let cur_buf = Buffer::current();
    let cur_win = Window::current();

    let Ok(position) = cur_win
        .get_cursor()
        .map(|(row, column)| Point { row, column })
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot get cursor from current window {cur_win:#?}, error {error:#?}"
            ));
        })
    else {
        return;
    };

    let Ok(file_path) = cur_buf
        .get_name()
        .map(|s| PathBuf::from(s.to_string_lossy().as_ref()))
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot get buffer name of buffer #{cur_buf:#?}, error {error:#?}"
            ));
        })
    else {
        return;
    };

    let Some(test_name) = get_enclosing_fn_name_of_position(&file_path, position)
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!("cannot get enclosing fn for {position:#?}, error {error:#?}"));
        })
        .ok()
        .flatten()
    else {
        crate::oxi_ext::api::notify_error(&format!("missing enclosing fn found for {position:#?}"));
        return;
    };

    let Ok(cur_pane_id) = ytil_wezterm::get_current_pane_id().inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("cannot get current `WezTerm` pane id, error {error:#?}"));
    }) else {
        return;
    };

    let Ok(wez_panes) = ytil_wezterm::get_all_panes(&[]).inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!("cannot get `WezTerm` panes, error {error:#?}"));
    }) else {
        return;
    };

    let Some(cur_pane) = wez_panes.iter().find(|p| p.pane_id == cur_pane_id) else {
        crate::oxi_ext::api::notify_error(&format!(
            "WezTerm pane with {cur_pane_id:#?} not found among panes {wez_panes:#?}"
        ));
        return;
    };

    let Some(test_runner_pane) = wez_panes.iter().find(|p| p.is_sibling_terminal_pane_of(cur_pane)) else {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot find a pane sibling to {cur_pane:#?} among `WezTerm` panes {wez_panes:#?} where to run the test {test_name}"
        ));
        return;
    };

    let Ok(test_runner_app) = get_test_runner_app_for_path(&file_path).inspect_err(|error| {
        crate::oxi_ext::api::notify_error(&format!(
            "cannot get test runner app for file {}, error {error:#?}",
            file_path.display()
        ));
    }) else {
        return;
    };

    let test_run_cmd = format!("'{test_runner_app} {test_name}'");
    let send_text_to_pane_cmd = ytil_wezterm::send_text_to_pane_cmd(&test_run_cmd, test_runner_pane.pane_id);
    let submit_pane_cmd = ytil_wezterm::submit_pane_cmd(test_runner_pane.pane_id);

    let Ok(_) = ytil_cmd::silent_cmd("sh")
        .args(["-c", &format!("{send_text_to_pane_cmd} && {submit_pane_cmd}")])
        .spawn()
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "error executing {test_run_cmd:#?} in `WezTerm` pane {test_runner_pane:#?}, error {error:#?}"
            ));
        })
    else {
        return;
    };
}

/// Gets the name of the function enclosing the given [Point] in a Rust file.
///
/// # Errors
///
/// Returns an error if:
/// - A filesystem operation (open/read/write/remove) fails.
fn get_enclosing_fn_name_of_position(file_path: &Path, position: Point) -> color_eyre::Result<Option<String>> {
    eyre::ensure!(
        file_path.extension().is_some_and(|ext| ext == "rs"),
        "{file_path:#?} must be Rust file"
    );
    let src = std::fs::read(file_path).with_context(|| format!("Error reading {}", file_path.display()))?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .with_context(|| "error setting parser language")?;

    let src_tree = parser
        .parse(&src, None)
        .ok_or_else(|| eyre!("error parsing src {} as Rust", file_path.display()))?;

    let node_at_position = src_tree.root_node().descendant_for_point_range(position, position);

    Ok(get_enclosing_fn_name_of_node(&src, node_at_position))
}

/// Gets the name of the function enclosing the given [Node].
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
///
/// # Errors
///
/// Returns an error if:
/// - A filesystem operation (open/read/write/remove) fails.
/// - The path is not inside a Git repository.
fn get_test_runner_app_for_path(path: &Path) -> color_eyre::Result<&'static str> {
    let git_repo_root = ytil_git::get_repo_root(&ytil_git::get_repo(path)?);

    if std::fs::read_dir(git_repo_root)?.any(|res| {
        res.as_ref()
            .map(|de| de.file_name() == "Makefile.toml")
            .unwrap_or(false)
    }) {
        return Ok("cargo make test");
    }

    Ok("cargo test")
}
