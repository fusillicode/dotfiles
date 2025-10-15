use nvim_oxi::Dictionary;
use serde::Deserialize;

use crate::diagnostics::DiagnosticSeverity;
use crate::dict;
use crate::fn_from;
use crate::oxi_ext::api::notify_error;
use crate::oxi_ext::api::notify_warn;

pub fn dict() -> Dictionary {
    dict! {
        "sqruff": dict! {
            "parser": fn_from!(parser)
        },
    }
}

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

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct SqruffOutput {
    #[serde(rename = "<string>", default)]
    messages: Vec<SqruffMessage>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct SqruffMessage {
    code: Option<String>,
    message: String,
    range: Range,
    severity: DiagnosticSeverity,
    source: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct Position {
    character: u32,
    line: u32,
}

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
        pretty_assertions::assert_eq!(expected, res);
    }

    #[test]
    fn sqruff_output_deserializes_empty_messages() {
        let value = serde_json::json!({
            "<string>": []
        });

        assert2::let_assert!(Ok(parsed) = serde_json::from_value::<SqruffOutput>(value));
        pretty_assertions::assert_eq!(SqruffOutput { messages: vec![] }, parsed);
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
            },
            res
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
            },
            res
        );
    }
}
