use std::path::Path;
use std::path::PathBuf;

const DEFAULT_COLUMN: usize = 1;
const DEFAULT_LINE: usize = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileTarget {
    path: PathBuf,
    line: usize,
    column: usize,
}

impl FileTarget {
    pub fn parse(input: &str) -> Option<Self> {
        let input = fold_path_match(input);
        let trimmed = input.trim_matches(is_path_edge_delimiter);
        if trimmed.is_empty() {
            return None;
        }

        let Some((head, trailing_number)) = split_trailing_number(trimmed) else {
            return Self::new(trimmed, DEFAULT_LINE, DEFAULT_COLUMN);
        };

        let (path, line, column) = if let Some((path, line)) = split_trailing_number(head) {
            (path, line, trailing_number.max(DEFAULT_COLUMN))
        } else {
            (head, trailing_number, DEFAULT_COLUMN)
        };

        Self::new(path, line, column)
    }

    fn new(path: &str, line: usize, column: usize) -> Option<Self> {
        if path.is_empty() || line == 0 || path.contains("://") {
            return None;
        }
        Some(Self {
            path: PathBuf::from(path),
            line,
            column,
        })
    }

    pub fn resolve(self, cwd: Option<&PathBuf>) -> Self {
        let Self { mut path, line, column } = self;
        if !path.is_absolute()
            && let Some(cwd) = cwd
        {
            path = cwd.join(path);
        }

        Self { path, line, column }
    }

    fn cursor_arg(&self) -> String {
        format!("+call cursor({}, {})", self.line, self.column)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn parent_path(&self) -> Option<&Path> {
        self.path.parent()
    }

    pub fn shell_cmd(&self, cwd: Option<&PathBuf>) -> String {
        let cd_prefix = cwd.map_or_else(String::new, |cwd| {
            format!("cd {} && ", single_quoted_string(&cwd.to_string_lossy(), "'\\''"))
        });
        format!(
            "{cd_prefix}nvim {} -- {}\r",
            single_quoted_string(&self.cursor_arg(), "'\\''"),
            single_quoted_string(&self.path.to_string_lossy(), "'\\''")
        )
    }

    pub fn edit_cmd(&self) -> String {
        format!(
            ":silent execute 'edit ' . fnameescape({}) | call cursor({}, {}) | redraw!\r",
            single_quoted_string(&self.path.to_string_lossy(), "''"),
            self.line,
            self.column
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum FileTargetReconstructed {
    NoMatch,
    Unique(FileTarget),
    Ambiguous,
}

impl FileTargetReconstructed {
    pub fn from_lines(lines: &[String], matched_string: &str) -> Self {
        let matched_string = matched_string.trim();
        if matched_string.is_empty() {
            return Self::NoMatch;
        }

        let text = lines.join("\n");
        let chars = text.char_indices().collect::<Vec<_>>();
        let mut found_target = None;
        for (match_start, match_text) in text.match_indices(matched_string) {
            let Some(match_end) = match_start.checked_add(match_text.len()) else {
                continue;
            };
            let Some((start, end)) = find_folded_path_bounds(&chars, match_start, match_end) else {
                continue;
            };

            let start_byte = chars.get(start).map_or(text.len(), |(idx, _ch)| *idx);
            let end_byte = chars.get(end).map_or(text.len(), |(idx, _ch)| *idx);
            let Some(candidate) = text.get(start_byte..end_byte) else {
                continue;
            };
            let folded = fold_path_match(candidate);
            if (folded.contains('/') || folded.starts_with('.') || split_trailing_number(&folded).is_some())
                && let Some(target) = FileTarget::parse(candidate)
            {
                let Some(existing) = found_target.as_ref() else {
                    found_target = Some(target);
                    continue;
                };
                if existing != &target {
                    return Self::Ambiguous;
                }
            }
        }

        found_target.map_or(Self::NoMatch, Self::Unique)
    }
}

fn fold_path_match(input: &str) -> String {
    input
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .collect()
}

fn is_path_edge_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            ':' | ';' | ',' | '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
        )
}

fn find_folded_path_bounds(chars: &[(usize, char)], match_start: usize, match_end: usize) -> Option<(usize, usize)> {
    let mut start = chars.partition_point(|(idx, _ch)| *idx < match_start);
    let end_idx = chars.partition_point(|(idx, _ch)| *idx < match_end);
    let mut end = end_idx;

    while start > 0 {
        let prev = start.saturating_sub(1);
        if !is_folded_path_char(chars, prev) {
            break;
        }
        start = prev;
    }

    while end < chars.len() && is_folded_path_char(chars, end) {
        end = end.saturating_add(1);
    }

    (start < end && start <= end_idx && end >= end_idx).then_some((start, end))
}

fn is_folded_path_char(chars: &[(usize, char)], idx: usize) -> bool {
    chars.get(idx).is_some_and(|(_byte_idx, ch)| is_path_body_char(*ch)) || is_folded_path_space(chars, idx)
}

fn is_folded_path_space(chars: &[(usize, char)], idx: usize) -> bool {
    let Some((_byte_idx, ch)) = chars.get(idx) else {
        return false;
    };
    if !ch.is_whitespace() && !ch.is_control() {
        return false;
    }

    let mut before = None;
    for candidate in (0..idx).rev() {
        let Some((_byte_idx, ch)) = chars.get(candidate) else {
            continue;
        };
        if !ch.is_whitespace() && !ch.is_control() {
            before = Some(*ch);
            break;
        }
    }

    let mut after = None;
    for candidate in idx.saturating_add(1)..chars.len() {
        let Some((_byte_idx, ch)) = chars.get(candidate) else {
            continue;
        };
        if !ch.is_whitespace() && !ch.is_control() {
            after = Some(*ch);
            break;
        }
    }

    before.is_some_and(|ch| matches!(ch, '/' | '-')) && after.is_some_and(is_path_body_char)
}

const fn is_path_body_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '/' | '.' | '_' | '-' | '+' | '@' | '%' | ',' | '#' | '=' | '~' | '!' | '$' | '{' | '}' | '[' | ']' | ':'
        )
}

fn split_trailing_number(input: &str) -> Option<(&str, usize)> {
    let colon_pos = input.rfind(':')?;
    let number = input.get(colon_pos.checked_add(1)?..)?;
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let head = input.get(..colon_pos)?;
    Some((head, number.parse().ok()?))
}

fn single_quoted_string(input: &str, escape: &str) -> String {
    let mut output = String::from("'");
    for ch in input.chars() {
        if ch == '\'' {
            output.push_str(escape);
        } else {
            output.push(ch);
        }
    }
    output.push('\'');
    output
}

#[cfg(test)]
mod file_target_tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use crate::wasm::file_target::FileTarget;
    use crate::wasm::file_target::FileTargetReconstructed;

    #[rstest]
    #[case("/tmp/foo.rs:42", Some(("/tmp/foo.rs", 42, 1)))]
    #[case("/tmp/foo.rs:42:9", Some(("/tmp/foo.rs", 42, 9)))]
    #[case("src/main.rs:7", Some(("src/main.rs", 7, 1)))]
    #[case("./src/main.rs:7", Some(("./src/main.rs", 7, 1)))]
    #[case(".env:7", Some((".env", 7, 1)))]
    #[case("Makefile:7", Some(("Makefile", 7, 1)))]
    #[case("src/main.rs", Some(("src/main.rs", 1, 1)))]
    #[case("Cargo", Some(("Cargo", 1, 1)))]
    #[case("(Cargo.toml),", Some(("Cargo.toml", 1, 1)))]
    #[case(":src/main.rs:", Some(("src/main.rs", 1, 1)))]
    #[case("::src/main.rs:7:", Some(("src/main.rs", 7, 1)))]
    #[case("<src/main.rs:7:9>,", Some(("src/main.rs", 7, 9)))]
    #[case("foo:bar.rs", Some(("foo:bar.rs", 1, 1)))]
    #[case("src/\n    main.rs:7", Some(("src/main.rs", 7, 1)))]
    #[case("configs/admin-\n    local.yaml:222", Some(("configs/admin-local.yaml", 222, 1)))]
    #[case("https://example.test/file.rs", None)]
    fn test_file_target_parse_returns_target(#[case] input: &str, #[case] expected: Option<(&str, usize, usize)>) {
        let expected = expected.map(|(path, line, column)| FileTarget {
            path: PathBuf::from(path),
            line,
            column,
        });

        pretty_assertions::assert_eq!(FileTarget::parse(input), expected);
    }

    #[rstest]
    #[case("src/main.rs", Some("/repo"), "/repo/src/main.rs")]
    #[case("/tmp/main.rs", Some("/repo"), "/tmp/main.rs")]
    #[case("src/main.rs", None, "src/main.rs")]
    fn test_file_target_resolve_returns_resolved_target(
        #[case] path: &str,
        #[case] cwd: Option<&str>,
        #[case] expected_path: &str,
    ) {
        let target = FileTarget {
            path: PathBuf::from(path),
            line: 7,
            column: 3,
        };
        let cwd = cwd.map(PathBuf::from);

        pretty_assertions::assert_eq!(
            target.resolve(cwd.as_ref()),
            FileTarget {
                path: PathBuf::from(expected_path),
                line: 7,
                column: 3,
            }
        );
    }

    #[rstest]
    #[case("/Users/gianlu/data/dev/work/earnings/earnings/")]
    #[case("earnings.review-feat-ENB-398-codex/kweb/configs/admin-")]
    #[case("local.yaml:222")]
    fn test_file_target_reconstructed_from_lines_returns_wrapped_target(#[case] clicked: &str) {
        let lines = [
            " - [warning] /Users/gianlu/data/dev/work/earnings/earnings/".to_owned(),
            "    earnings.review-feat-ENB-398-codex/kweb/configs/admin-".to_owned(),
            "    local.yaml:222 - local admin config enable".to_owned(),
        ];

        pretty_assertions::assert_eq!(
            FileTargetReconstructed::from_lines(&lines, clicked),
            FileTargetReconstructed::Unique(FileTarget {
                path: PathBuf::from(
                    "/Users/gianlu/data/dev/work/earnings/earnings/earnings.review-feat-ENB-398-codex/kweb/configs/admin-local.yaml"
                ),
                line: 222,
                column: 1,
            })
        );
    }

    #[test]
    fn test_file_target_reconstructed_from_lines_ignores_text_outside_path() {
        let lines = [
            " - [warning] /Users/gianlu/data/dev/work/earnings/earnings/".to_owned(),
            "    earnings.review-feat-ENB-398-codex/kweb/configs/admin-".to_owned(),
            "    local.yaml:222 - local admin config enable".to_owned(),
        ];

        pretty_assertions::assert_eq!(
            FileTargetReconstructed::from_lines(&lines, "warning"),
            FileTargetReconstructed::NoMatch
        );
    }

    #[test]
    fn test_file_target_reconstructed_from_lines_returns_ambiguous_for_multiple_targets() {
        let lines = [
            "/tmp/first/local.yaml:222".to_owned(),
            "/tmp/second/local.yaml:222".to_owned(),
        ];

        pretty_assertions::assert_eq!(
            FileTargetReconstructed::from_lines(&lines, "local.yaml:222"),
            FileTargetReconstructed::Ambiguous
        );
    }

    #[rstest]
    #[case(
        "/tmp/foo'bar.rs",
        ":silent execute 'edit ' . fnameescape('/tmp/foo''bar.rs') | call cursor(12, 3) | redraw!\r"
    )]
    fn test_file_target_edit_cmd_returns_nvim_command(#[case] path: &str, #[case] expected: &str) {
        let target = FileTarget {
            path: PathBuf::from(path),
            line: 12,
            column: 3,
        };

        pretty_assertions::assert_eq!(target.edit_cmd(), expected);
    }

    #[rstest]
    #[case("/tmp/foo'bar.rs", None, "nvim '+call cursor(12, 3)' -- '/tmp/foo'\\''bar.rs'\r")]
    #[case(
        "/repo/src/main.rs",
        Some("/repo"),
        "cd '/repo' && nvim '+call cursor(12, 3)' -- '/repo/src/main.rs'\r"
    )]
    fn test_file_target_shell_cmd_returns_nvim_command(
        #[case] path: &str,
        #[case] cwd: Option<&str>,
        #[case] expected: &str,
    ) {
        let target = FileTarget {
            path: PathBuf::from(path),
            line: 12,
            column: 3,
        };
        let cwd = cwd.map(PathBuf::from);

        pretty_assertions::assert_eq!(target.shell_cmd(cwd.as_ref()), expected);
    }
}
