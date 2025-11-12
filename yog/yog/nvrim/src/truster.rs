//! Rust test runner helpers integrating with Nvim.
//!
//! Exposes a dictionary enabling cursor-aware test execution (`run_test`) by parsing the current buffer
//! with Tree‑sitter to locate the nearest test function and spawning it inside a WezTerm pane.
//! All Nvim API failures are reported via [`ytil_nvim_oxi::api::notify_error`].

use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;
use ytil_nvim_oxi::buffer::CursorPosition;

/// [`Dictionary`] of Rust tests utilities.
pub fn dict() -> Dictionary {
    dict! {
        "run_test": fn_from!(run_test),
    }
}

/// Runs the test function at the current cursor position in a `WezTerm` pane.
fn run_test(_: ()) {
    let Some(position) = CursorPosition::get_current().map(PointWrap::from) else {
        return;
    };
    let cur_buf = Buffer::current();

    let Ok(file_path) = cur_buf
        .get_name()
        .map(|s| PathBuf::from(s.to_string_lossy().as_ref()))
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!("cannot get buffer name | buffer={cur_buf:#?} error={error:#?}"));
        })
    else {
        return;
    };

    let Some(test_name) = get_enclosing_fn_name_of_position(&file_path, *position)
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!(
                "cannot get enclosing fn | position={position:#?} error={error:#?}"
            ));
        })
        .ok()
        .flatten()
    else {
        ytil_nvim_oxi::api::notify_error(format!("missing enclosing fn | position={position:#?}"));
        return;
    };

    let Ok(cur_pane_id) = ytil_wezterm::get_current_pane_id().inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get current `WezTerm` pane id | error={error:#?}"));
    }) else {
        return;
    };

    let Ok(wez_panes) = ytil_wezterm::get_all_panes(&[]).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!("cannot get `WezTerm` panes | error={error:#?}"));
    }) else {
        return;
    };

    let Some(cur_pane) = wez_panes.iter().find(|p| p.pane_id == cur_pane_id) else {
        ytil_nvim_oxi::api::notify_error(format!(
            "wezterm pane not found | pane_id={cur_pane_id:#?} panes={wez_panes:#?}"
        ));
        return;
    };

    let Some(test_runner_pane) = wez_panes.iter().find(|p| p.is_sibling_terminal_pane_of(cur_pane)) else {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot find sibling pane to run test | current_pane={cur_pane:#?} panes={wez_panes:#?} test={test_name}"
        ));
        return;
    };

    let Ok(test_runner_app) = get_test_runner_app_for_path(&file_path).inspect_err(|error| {
        ytil_nvim_oxi::api::notify_error(format!(
            "cannot get test runner app | path={} error={error:#?}",
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
            ytil_nvim_oxi::api::notify_error(format!(
                "cannot execute test run cmd | cmd={test_run_cmd:#?} pane={test_runner_pane:#?} error={error:#?}"
            ));
        })
    else {
        return;
    };
}

/// Wrapper around [`tree_sitter::Point`] that converts Nvim's 1-based row indexing
/// to tree-sitter's 0-based indexing.
///
/// # Rationale
///
/// Nvim uses 1-based row indices for cursor positions, while tree-sitter expects 0-based rows.
/// This wrapper simplifies the conversion in the codebase.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
struct PointWrap(Point);

impl Deref for PointWrap {
    type Target = Point;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<CursorPosition> for PointWrap {
    /// Converts a Nvim cursor position (1-based row, 0-based column) to a [`PointWrap`].
    ///
    /// # Arguments
    /// - `row` 1-based row index from Nvim.
    /// - `column` 0-based column index from Nvim.
    ///
    /// # Returns
    /// A [`PointWrap`] with 0-based row and column suitable for Tree-sitter.
    fn from(cursor_position: CursorPosition) -> Self {
        Self(Point {
            row: cursor_position.row.saturating_sub(1),
            column: cursor_position.col,
        })
    }
}

/// Gets the name of the function enclosing the given [Point] in a Rust file.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
fn get_enclosing_fn_name_of_position(file_path: &Path, position: Point) -> color_eyre::Result<Option<String>> {
    eyre::ensure!(
        file_path.extension().is_some_and(|ext| ext == "rs"),
        "invalid file extension | path={file_path:?} expected_ext=rs"
    );
    let src = std::fs::read(file_path).with_context(|| format!("Error reading {}", file_path.display()))?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .with_context(|| "error setting parser language")?;

    let src_tree = parser
        .parse(&src, None)
        .ok_or_else(|| eyre!("rust parse failed | path={}", file_path.display()))?;

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(CursorPosition { row:1, col: 5}, (0, 5))]
    #[case(CursorPosition { row:10, col: 20}, (9, 20))]
    #[case(CursorPosition { row:0, col: 0}, (0, 0))]
    fn point_wrap_from_converts_neovim_cursor_to_tree_sitter_point(
        #[case] input: CursorPosition,
        #[case] expected: (usize, usize),
    ) {
        pretty_assertions::assert_eq!(
            PointWrap::from(input),
            PointWrap(Point {
                row: expected.0,
                column: expected.1
            })
        );
    }

    #[test]
    fn point_wrap_deref_allows_direct_access_to_point() {
        pretty_assertions::assert_eq!(
            *PointWrap::from(CursorPosition { row: 5, col: 10 }),
            Point { row: 4, column: 10 }
        );
    }

    #[test]
    fn get_enclosing_fn_name_of_node_returns_fn_name_when_inside_function() {
        let result = with_node(
            b"fn test_function() { let x = 1; }",
            Point { row: 0, column: 20 },
            get_enclosing_fn_name_of_node,
        );
        pretty_assertions::assert_eq!(result, Some("test_function".to_string()));
    }

    #[test]
    fn get_enclosing_fn_name_of_node_returns_none_when_not_inside_function() {
        let result = with_node(
            b"let x = 1;",
            Point { row: 0, column: 5 },
            get_enclosing_fn_name_of_node,
        );
        pretty_assertions::assert_eq!(result, None);
    }

    #[test]
    fn get_enclosing_fn_name_of_node_returns_method_name_when_inside_method() {
        let result = with_node(
            b"impl Test { fn method(&self) { let x = 1; } }",
            Point { row: 0, column: 25 },
            get_enclosing_fn_name_of_node,
        );
        pretty_assertions::assert_eq!(result, Some("method".to_string()));
    }

    #[test]
    fn get_enclosing_fn_name_of_node_returns_none_when_node_is_none() {
        let result = get_enclosing_fn_name_of_node(b"fn test() {}", None);
        pretty_assertions::assert_eq!(result, None);
    }

    // Helper to work around the [`tree_sitter::Tree`] and [`tree_sitter::Node`] lifetime issues.
    fn with_node<F, R>(src: &[u8], position: Point, f: F) -> R
    where
        F: FnOnce(&[u8], Option<Node>) -> R,
    {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let node = tree.root_node().descendant_for_point_range(position, position);
        f(src, node)
    }
}
