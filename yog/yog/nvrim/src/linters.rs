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
struct SqruffOutput {
    #[serde(rename = "<string>", default)]
    messages: Vec<SqruffMessage>,
}

#[derive(Debug, Deserialize)]
struct SqruffMessage {
    code: Option<String>,
    message: String,
    range: Range,
    severity: DiagnosticSeverity,
    source: String,
}

#[derive(Debug, Deserialize)]
struct Range {
    start: Position,
    end: Position,
}

#[derive(Debug, Deserialize)]
struct Position {
    character: u32,
    line: u32,
}

fn diagnostic_dict_from_msg(msg: SqruffMessage) -> Dictionary {
    dict! {
        "lnum": msg.range.start.line.saturating_sub(1) as i64,
        "end_lnum": msg.range.end.line.saturating_sub(1) as i64,
        "col": msg.range.start.character.saturating_sub(1) as i64,
        "end_col": msg.range.end.character.saturating_sub(1) as i64,
        "message": msg.message,
        "code": msg.code.map(nvim_oxi::Object::from).unwrap_or(nvim_oxi::Object::nil()),
        "source": msg.source,
        "severity": msg.severity as u8,
    }
}
