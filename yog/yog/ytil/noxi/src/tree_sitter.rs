use std::ops::Deref;
use std::path::Path;

use rootcause::prelude::ResultExt as _;
use rootcause::report;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Point;

use crate::buffer::CursorPosition;

/// Wrapper around [`tree_sitter::Point`] converting Nvim's 1-based row to 0-based.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct PointWrap(Point);

impl Deref for PointWrap {
    type Target = Point;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<CursorPosition> for PointWrap {
    /// Converts a Nvim cursor position (1-based row, 0-based column) to a [`PointWrap`].
    fn from(cursor_position: CursorPosition) -> Self {
        Self(Point {
            row: cursor_position.row.saturating_sub(1),
            column: cursor_position.col,
        })
    }
}

pub fn get_enclosing_fn_name_of_position(file_path: &Path) -> Option<String> {
    let position = CursorPosition::get_current().map(PointWrap::from)?;

    let enclosing_fn_name = get_enclosing_fn_name_of_position_internal(file_path, *position)
        .inspect_err(|err| {
            crate::notify::error(format!(
                "error getting enclosing fn | position={position:#?} error={err:#?}"
            ));
        })
        .ok()
        .flatten();

    if enclosing_fn_name.is_none() {
        crate::notify::error(format!("error missing enclosing fn | position={position:#?}"));
    }

    enclosing_fn_name
}

/// Gets the name of the function enclosing the given [Point] in a Rust file.
///
/// # Errors
/// - A filesystem operation (open/read/write/remove) fails.
fn get_enclosing_fn_name_of_position_internal(file_path: &Path, position: Point) -> rootcause::Result<Option<String>> {
    if file_path.extension().is_none_or(|ext| ext != "rs") {
        Err(report!("invalid file extension"))
            .attach_with(|| format!("path={} expected_ext=\"rs\"", file_path.display()))?;
    }
    let src = std::fs::read(file_path)
        .context("error reading file")
        .attach_with(|| format!("path={}", file_path.display()))?;

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("error setting parser language")?;

    let src_tree = parser
        .parse(&src, None)
        .ok_or_else(|| report!("error parsing Rust code"))
        .attach_with(|| format!("path={}", file_path.display()))?;

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
