//! Linter parsing helpers.
//!
//! - Provides an extensible registry of linter parser functions (see [`dict`]).
//! - Currently ships a hardened `sqruff` parser exposed under the `sqruff` table.
//! - Parses tool JSON output into Neovim diagnostic dictionaries.
//! - Designed so additional linters can be added with minimal boilerplate.
//!
//! # Rationale
//! - Introduced after the upstream `nvim-lint` default `sqruff` parser failed to handle `vim.NIL`, crashing the Neovim
//!   instance. This implementation treats absence / `nil` values defensively and never propagates a panic.
//! - Centralizing parsers allows consistent error reporting & future sharing of common normalization logic (e.g.
//!   severity mapping, range sanitation).

use nvim_oxi::Dictionary;
use serde::Deserialize;

use crate::diagnostics::DiagnosticSeverity;
use crate::dict;
use crate::fn_from;
use crate::oxi_ext::api::notify_error;
use crate::oxi_ext::api::notify_warn;

/// [`Dictionary`] of linters parsers.
pub fn dict() -> Dictionary {
    dict! {
        "sqruff": dict! {
            "parser": fn_from!(parser)
        },
    }
}

/// Parse raw `sqruff` JSON output into Neovim diagnostic [`Dictionary`].
///
/// # Behavior
/// - Empty / missing input: returns an empty vector and emits a warning.
/// - Malformed JSON: returns an empty vector and emits an error notification.
/// - Successful parse: converts each message into a diagnostic `Dictionary`.
///
/// # Arguments
/// - `maybe_output` Optional Neovim string containing the `sqruff` JSON payload.
///
/// # Returns
/// Vector of diagnostic [`Dictionary`]'s consumable by Neovim.
#[allow(clippy::needless_pass_by_value)]
fn parser(maybe_output: Option<nvim_oxi::String>) -> Vec<Dictionary> {
    let Some(output) = &maybe_output else {
        notify_warn(&format!("sqruff output missing output={maybe_output:?}"));
        return vec![];
    };
    let output = output.to_string_lossy();

    if output.trim().is_empty() {
        notify_warn(&format!("sqruff output is an empty string output={maybe_output:?}"));
        return vec![];
    }

    let parsed_output = match serde_json::from_str::<SqruffOutput>(&output) {
        Ok(parsed_output) => parsed_output,
        Err(error) => {
            notify_error(&format!("error parsing sqruff output={output:?} error={error:#?}"));
            return vec![];
        }
    };

    parsed_output
        .messages
        .into_iter()
        .map(diagnostic_dict_from_msg)
        .collect()
}

/// Parsed `sqruff` top-level output structure.
///
/// - Holds a list of lint messages under the JSON key `<string>` produced by `sqruff`.
///
/// # Rationale
/// Mirrors the external tool's JSON so deserialization stays trivial and
/// downstream logic can iterate messages directly.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct SqruffOutput {
    #[serde(rename = "<string>", default)]
    messages: Vec<SqruffMessage>,
}

/// Single `sqruff` lint message entry.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct SqruffMessage {
    /// Optional rule identifier emitted by `sqruff`.
    code: Option<String>,
    /// Human-readable lint explanation.
    message: String,
    /// 1-based inclusive start / exclusive end span reported by the tool.
    range: Range,
    /// Lint severity expressed as [`DiagnosticSeverity`].
    severity: DiagnosticSeverity,
    /// Source tool identifier (always `sqruff`).
    source: String,
}

/// Source span covering the offending text range.
///
/// # Rationale
/// Explicit start/end structs keep deserialization unambiguous and allow
/// saturation adjustments when converting to line/column indices expected by
/// Neovim (0-based internally after adjustment).
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Range {
    /// Start position (1-based line / column as emitted by `sqruff`).
    start: Position,
    /// End position (1-based line / column, exclusive column semantics when
    /// converted to Neovim diagnostics after 0-based adjustment).
    end: Position,
}

/// Line/column pair (1-based as emitted by `sqruff`).
///
/// # Rationale
/// Maintains external numbering; translation to 0-based indices occurs when
/// building Neovim dictionaries.
#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Position {
    /// 1-based column number.
    character: u32,
    /// 1-based line number.
    line: u32,
}

/// Convert a single [`SqruffMessage`] into a Neovim [`Dictionary`].
///
/// # Arguments
/// - `msg` The parsed `sqruff` message.
///
/// # Returns
/// A [`Dictionary`] keyed with fields `lnum`, `col`, `message`, `code`,
/// `source`, `severity`.
fn diagnostic_dict_from_msg(msg: SqruffMessage) -> Dictionary {
    dict! {
        "lnum": msg.range.start.line.saturating_sub(1),
        "end_lnum": msg.range.end.line.saturating_sub(1),
        "col": msg.range.start.character.saturating_sub(1),
        "end_col": msg.range.end.character.saturating_sub(1),
        "message": msg.message,
        "code": msg.code.map_or_else(nvim_oxi::Object::nil, nvim_oxi::Object::from),
        "source": msg.source,
        "severity": msg.severity.to_number(),
    }
}

#[cfg(test)]
mod tests {
    use nvim_oxi::Object;

    use super::*;

    #[test]
    fn diagnostic_dict_from_msg_returns_the_expected_dict_from_msg() {
        let msg = SqruffMessage {
            code: Some("R001".to_string()),
            message: "Example message".to_string(),
            range: Range {
                start: Position { line: 3, character: 7 },
                end: Position { line: 4, character: 10 },
            },
            severity: DiagnosticSeverity::Warn,
            source: "sqruff".to_string(),
        };

        let res = diagnostic_dict_from_msg(msg);

        let expected = dict! {
            "lnum": 2,
            "end_lnum": 3,
            "col": 6,
            "end_col": 9,
            "message": "Example message".to_string(),
            "code": Object::from(nvim_oxi::String::from("R001")),
            "source": "sqruff".to_string(),
            "severity": DiagnosticSeverity::Warn.to_number(),
        };
        pretty_assertions::assert_eq!(res, expected);
    }

    #[test]
    fn sqruff_output_deserializes_empty_messages() {
        let value = serde_json::json!({
            "<string>": []
        });

        assert2::let_assert!(Ok(parsed) = serde_json::from_value::<SqruffOutput>(value));
        pretty_assertions::assert_eq!(parsed, SqruffOutput { messages: vec![] });
    }

    #[test]
    fn sqruff_output_deserializes_single_message_with_code() {
        let value = serde_json::json!({
            "<string>": [
                {
                    "code": "R001",
                    "message": "Msg",
                    "range": {"start": {"line": 2, "character": 5}, "end": {"line": 2, "character": 10}},
                    "severity": "2",
                    "source": "sqruff"
                }
            ]
        });

        assert2::let_assert!(Ok(res) = serde_json::from_value::<SqruffOutput>(value));
        pretty_assertions::assert_eq!(
            res,
            SqruffOutput {
                messages: vec![SqruffMessage {
                    code: Some("R001".into()),
                    message: "Msg".into(),
                    range: Range {
                        start: Position { line: 2, character: 5 },
                        end: Position { line: 2, character: 10 },
                    },
                    severity: DiagnosticSeverity::Warn,
                    source: "sqruff".into(),
                }],
            }
        );
    }

    #[test]
    fn sqruff_output_deserializes_multiple_messages_mixed_code() {
        let value = serde_json::json!({
            "<string>": [
                {
                    "code": "R001",
                    "message": "HasCode",
                    "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}},
                    "severity": "2",
                    "source": "sqruff"
                },
                {
                    "code": null,
                    "message": "NoCode",
                    "range": {"start": {"line": 1, "character": 1}, "end": {"line": 1, "character": 2}},
                    "severity": "1",
                    "source": "sqruff"
                }
            ]
        });

        assert2::let_assert!(Ok(res) = serde_json::from_value::<SqruffOutput>(value));
        pretty_assertions::assert_eq!(
            res,
            SqruffOutput {
                messages: vec![
                    SqruffMessage {
                        code: Some("R001".into()),
                        message: "HasCode".into(),
                        range: Range {
                            start: Position { line: 3, character: 7 },
                            end: Position { line: 3, character: 12 },
                        },
                        severity: DiagnosticSeverity::Warn,
                        source: "sqruff".into(),
                    },
                    SqruffMessage {
                        code: None,
                        message: "NoCode".into(),
                        range: Range {
                            start: Position { line: 1, character: 1 },
                            end: Position { line: 1, character: 2 },
                        },
                        severity: DiagnosticSeverity::Error,
                        source: "sqruff".into(),
                    },
                ],
            }
        );
    }
}
