use std::borrow::Cow;
use std::collections::HashMap;

use itertools::Itertools;
use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;
use strum::IntoEnumIterator;

use crate::diagnostics::DiagnosticSeverity;
use crate::dict;
use crate::fn_from;
use crate::oxi_ext::buffer::CursorPosition;

const DRAW_TRIGGERS: &[&str] = &["DiagnosticChanged", "BufEnter", "CursorMoved"];

/// [`Dictionary`] exposing statusline draw helpers.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
        "draw_triggers": DRAW_TRIGGERS.iter().map(ToString::to_string).collect::<Object>()
    }
}

/// Draws the status line with diagnostic information.
fn draw(diagnostics: Vec<Diagnostic>) -> Option<String> {
    let cur_buf = nvim_oxi::api::get_current_buf();
    let cur_buf_path = cur_buf
        .get_name()
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot get name of current buffer | buffer={cur_buf:#?} error={error:#?}"
            ));
        })
        .ok()?;
    let cwd = nvim_oxi::api::call_function::<_, String>("getcwd", Array::new())
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!("cannot get cwd | error={error:#?}"));
        })
        .ok()?;
    let cur_buf_path = cur_buf_path.to_string_lossy();

    let cur_buf_nr = cur_buf.handle();
    let mut statusline = Statusline {
        cur_buf_path: Cow::Borrowed(cur_buf_path.trim_start_matches(&cwd)),
        cur_buf_diags: HashMap::new(),
        workspace_diags: HashMap::new(),
        cursor_position: CursorPosition::get_current()?,
    };
    for diagnostic in diagnostics {
        if cur_buf_nr == diagnostic.bufnr {
            statusline
                .cur_buf_diags
                .entry(diagnostic.severity)
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
        }
        statusline
            .workspace_diags
            .entry(diagnostic.severity)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
    }

    Some(statusline.draw())
}

/// Represents a diagnostic from Nvim.
#[derive(Deserialize)]
pub struct Diagnostic {
    /// The buffer number.
    bufnr: i32,
    /// The severity of the diagnostic.
    severity: DiagnosticSeverity,
}

/// Implementation of [`FromObject`] for [`Diagnostic`].
impl FromObject for Diagnostic {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`nvim_oxi::lua::Poppable`] for [`Diagnostic`].
impl nvim_oxi::lua::Poppable for Diagnostic {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Represents the status line with buffer path and diagnostics.
#[derive(Debug)]
struct Statusline<'a> {
    /// The current buffer path.
    cur_buf_path: Cow<'a, str>,
    /// Diagnostics for the current buffer.
    cur_buf_diags: HashMap<DiagnosticSeverity, i32>,
    /// Diagnostics for the workspace.
    workspace_diags: HashMap<DiagnosticSeverity, i32>,
    cursor_position: CursorPosition,
}

impl Statusline<'_> {
    /// Draws the status line as a formatted string.
    ///
    /// Invariants:
    /// - Severity ordering stability depends on the declaration order of variants in [`DiagnosticSeverity`] and its
    ///   [`strum::IntoEnumIterator`] implementation. Changing the enum variant order will change rendering order and
    ///   break ordering tests.
    /// - Zero-count severities are omitted entirely (see `draw_diagnostics`).
    ///
    /// If this ordering or layout changes, update the focused tests in `statusline::tests` whose
    /// names encode the contract for clarity.
    fn draw(&self) -> String {
        let mut cur_buf_diags = DiagnosticSeverity::iter()
            .filter_map(|s| self.cur_buf_diags.get(&s).map(|c| draw_diagnostics(s, *c)))
            .join(" ");

        let workspace_diags = DiagnosticSeverity::iter()
            .filter_map(|s| self.workspace_diags.get(&s).map(|c| draw_diagnostics(s, *c)))
            .join(" ");

        if !cur_buf_diags.is_empty() {
            cur_buf_diags.push(' ');
        }

        format!(
            "{cur_buf_diags}%#StatusLine#{} {}:{} %m %r%={workspace_diags}",
            self.cur_buf_path, self.cursor_position.row, self.cursor_position.col
        )
    }
}

/// Draws the diagnostic count for this severity.
fn draw_diagnostics(severity: DiagnosticSeverity, diags_count: i32) -> String {
    if diags_count == 0 {
        return String::new();
    }
    let hg_group = match severity {
        DiagnosticSeverity::Error => "DiagnosticStatusLineError",
        DiagnosticSeverity::Warn => "DiagnosticStatusLineWarn",
        DiagnosticSeverity::Info => "DiagnosticStatusLineInfo",
        DiagnosticSeverity::Hint | DiagnosticSeverity::Other => "DiagnosticStatusLineHint",
    };
    format!("%#{hg_group}#{severity}:{diags_count}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test naming conventions enforced: subject_when_<condition>_<expected_outcome>

    #[test]
    fn statusline_draw_when_all_diagnostics_absent_or_zero_renders_plain_statusline() {
        for statusline in [
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: HashMap::new(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
                workspace_diags: HashMap::new(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
                workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
        ] {
            pretty_assertions::assert_eq!("%#StatusLine#foo 42:7 %m %r%=", statusline.draw());
        }
    }

    #[test]
    fn statusline_draw_when_current_buffer_has_diagnostics_renders_buffer_prefix() {
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            format!(
                "%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1 %#StatusLine#foo 42:7 %m %r%=",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
            statusline.draw()
        );
    }

    #[test]
    fn statusline_draw_when_workspace_has_diagnostics_renders_workspace_suffix() {
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            format!(
                "%#StatusLine#foo 42:7 %m %r%=%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
            statusline.draw()
        );
    }

    #[test]
    fn statusline_draw_when_both_buffer_and_workspace_have_diagnostics_renders_both_prefix_and_suffix() {
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(DiagnosticSeverity::Hint, 3), (DiagnosticSeverity::Warn, 2)]
                .into_iter()
                .collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            format!(
                "%#DiagnosticStatusLineWarn#{}:2 %#DiagnosticStatusLineHint#{}:3 %#StatusLine#foo 42:7 %m %r%=%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1",
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Hint,
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
            statusline.draw()
        );
    }

    #[test]
    fn statusline_draw_when_buffer_diagnostics_inserted_unordered_orders_by_severity() {
        // Insert in non-canonical order (Hint before Warn) and ensure output orders by severity (Warn then Hint).
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(DiagnosticSeverity::Hint, 5), (DiagnosticSeverity::Warn, 1)]
                .into_iter()
                .collect(),
            workspace_diags: HashMap::new(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            format!(
                "%#DiagnosticStatusLineWarn#{}:1 %#DiagnosticStatusLineHint#{}:5 %#StatusLine#foo 42:7 %m %r%=",
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Hint
            ),
            statusline.draw()
        );
    }

    #[test]
    fn draw_diagnostics_when_zero_count_returns_empty_string() {
        // Any severity with zero count should yield empty string.
        pretty_assertions::assert_eq!(String::new(), draw_diagnostics(DiagnosticSeverity::Error, 0));
        pretty_assertions::assert_eq!(String::new(), draw_diagnostics(DiagnosticSeverity::Warn, 0));
        pretty_assertions::assert_eq!(String::new(), draw_diagnostics(DiagnosticSeverity::Info, 0));
        pretty_assertions::assert_eq!(String::new(), draw_diagnostics(DiagnosticSeverity::Hint, 0));
    }

    #[test]
    fn statusline_draw_when_all_severity_counts_present_orders_buffer_and_workspace_diagnostics_by_severity() {
        // Insert diagnostics in deliberately scrambled order to validate deterministic ordering.
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [
                (DiagnosticSeverity::Hint, 1),
                (DiagnosticSeverity::Error, 4),
                (DiagnosticSeverity::Info, 2),
                (DiagnosticSeverity::Warn, 3),
            ]
            .into_iter()
            .collect(),
            workspace_diags: [
                (DiagnosticSeverity::Warn, 7),
                (DiagnosticSeverity::Info, 6),
                (DiagnosticSeverity::Hint, 5),
                (DiagnosticSeverity::Error, 8),
            ]
            .into_iter()
            .collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            format!(
                "%#DiagnosticStatusLineError#{}:4 %#DiagnosticStatusLineWarn#{}:3 %#DiagnosticStatusLineInfo#{}:2 %#DiagnosticStatusLineHint#{}:1 %#StatusLine#foo 42:7 %m %r%=%#DiagnosticStatusLineError#{}:8 %#DiagnosticStatusLineWarn#{}:7 %#DiagnosticStatusLineInfo#{}:6 %#DiagnosticStatusLineHint#{}:5",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Info,
                DiagnosticSeverity::Hint,
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Info,
                DiagnosticSeverity::Hint
            ),
            statusline.draw()
        );
    }
}
