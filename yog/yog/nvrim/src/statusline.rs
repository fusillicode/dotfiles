//! Statusline drawing helpers with diagnostics aggregation.
//!
//! Provides `statusline.dict()` with a `draw` function combining cwd, buffer name, cursor position and
//! LSP diagnostic severities / counts into a formatted status line; failures yield `None` and are
//! surfaced through [`crate::oxi_ext::api::notify_error`].

use std::borrow::Cow;

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
///
/// # Returns
/// - `Some(String)`: formatted statusline when buffer name, cwd, and cursor position retrieval succeed.
/// - `None`: if any prerequisite retrieval fails (buffer name, cwd, or cursor position). An error is logged via
///   [`crate::oxi_ext::api::notify_error`].
///
/// # Rationale
/// Returning `None` lets callers distinguish between a valid (possibly empty diagnostics) statusline and a data
/// acquisition failure.
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
        cur_buf_diags: SeverityBuckets::default(),
        workspace_diags: SeverityBuckets::default(),
        cursor_position: CursorPosition::get_current()?,
    };
    for diagnostic in diagnostics {
        statusline.workspace_diags.inc(diagnostic.severity);
        if cur_buf_nr == diagnostic.bufnr {
            statusline.cur_buf_diags.inc(diagnostic.severity);
        }
    }

    Some(statusline.draw())
}

/// Diagnostic emitted by Neovim.
///
/// Captures buffer association and severity for aggregation in the statusline.
///
/// # Rationale
/// Minimal fields keep deserialization lean; position, message, etc. are not needed for summary counts.
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

/// Fixed-size aggregation of counts per [`DiagnosticSeverity`].
///
/// Stores counts in an array indexed by a stable ordering declared by [`DiagnosticSeverity`] count.
/// Iteration yields (severity, count) pairs.
#[derive(Clone, Copy, Debug, Default)]
struct SeverityBuckets {
    counts: [i32; DiagnosticSeverity::VARIANT_COUNT],
}

impl SeverityBuckets {
    /// Increment severity count with saturating add.
    fn inc(&mut self, sev: DiagnosticSeverity) {
        let idx = sev as usize;
        if let Some(slot) = self.counts.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
    }

    /// Get count for severity.
    fn get(&self, sev: DiagnosticSeverity) -> i32 {
        let idx = sev as usize;
        self.counts.get(idx).copied().unwrap_or(0)
    }

    /// Iterate over (severity, count) pairs in canonical order (enum variant order per `EnumIter`).
    fn iter(&self) -> impl Iterator<Item = (DiagnosticSeverity, i32)> + '_ {
        DiagnosticSeverity::iter().map(|s| (s, self.get(s)))
    }

    /// Returns true if any count > 0.
    fn any(&self) -> bool {
        self.counts.iter().any(|&c| c > 0)
    }
}

/// Allow tests to build buckets from iterator of (severity, count).
impl FromIterator<(DiagnosticSeverity, i32)> for SeverityBuckets {
    fn from_iter<T: IntoIterator<Item = (DiagnosticSeverity, i32)>>(iter: T) -> Self {
        let mut buckets = Self::default();
        for (sev, count) in iter {
            let idx = sev as usize;
            if let Some(slot) = buckets.counts.get_mut(idx) {
                *slot = count; // Accept last-wins; tests construct unique severities
            }
        }
        buckets
    }
}

/// Represents the status line with buffer path and diagnostics.
#[derive(Debug)]
struct Statusline<'a> {
    /// The current buffer path.
    cur_buf_path: Cow<'a, str>,
    /// Diagnostics for the current buffer.
    cur_buf_diags: SeverityBuckets,
    /// Diagnostics for the workspace.
    workspace_diags: SeverityBuckets,
    /// Current cursor position used to render the trailing `row:col` segment.
    cursor_position: CursorPosition,
}

impl Statusline<'_> {
    /// Draws the status line as a formatted string.
    ///
    /// Invariants:
    /// - Severity ordering stability defined by [`DiagnosticSeverity`] enum variants order.
    /// - Zero-count severities are omitted (see [`draw_diagnostics`]).
    /// - Column displayed is 1-based via [`CursorPosition::adjusted_col`].
    /// - Row/column segment rendered as `row:col`.
    /// - A `%#StatusLine#` highlight reset precedes the position segment.
    fn draw(&self) -> String {
        let mut cur_buf_segment = self
            .cur_buf_diags
            .iter()
            .filter(|&(_, c)| c > 0)
            .map(draw_diagnostics)
            .join(" ");
        if self.cur_buf_diags.any() {
            cur_buf_segment.push(' '); // maintain previous trailing space contract
        }
        let workspace_segment = self
            .workspace_diags
            .iter()
            .filter(|&(_, c)| c > 0)
            .map(draw_diagnostics)
            .join(" ");

        format!(
            "{cur_buf_segment}%#StatusLine#{} %m %r%={workspace_segment}%#StatusLine# {}:{}",
            self.cur_buf_path,
            self.cursor_position.row,
            self.cursor_position.adjusted_col()
        )
    }
}

/// Draws the diagnostic count for a (severity, count) pair.
///
/// Accepts a tuple so it can be passed directly to iterator adapters like `.map(draw_diagnostics)` without
/// additional closure wrapping.
///
/// # Returns
/// - An empty [`String`] when count == 0 so zero-count severities can be filtered out upstream.
/// - A formatted segment `%#<HlGroup>#<severity>:<count>` otherwise.
///
/// # Rationale
/// Tuple parameter matches iterator `(DiagnosticSeverity, i32)` item shape, removing a tiny layer of syntactic noise
/// (`.map(|(s,c)| draw_diagnostics(s,c))`). Keeping zero-elision here is a harmless guard.
fn draw_diagnostics((severity, diags_count): (DiagnosticSeverity, i32)) -> String {
    if diags_count == 0 {
        return String::new();
    }
    let hg_group_dyn_part = match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warn => "Warn",
        DiagnosticSeverity::Info => "Info",
        DiagnosticSeverity::Hint | DiagnosticSeverity::Other => "Hint",
    };
    format!("%#DiagnosticStatusLine{hg_group_dyn_part}#{severity}:{diags_count}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statusline_draw_when_all_diagnostics_absent_or_zero_renders_plain_statusline() {
        for statusline in [
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: SeverityBuckets::default(),
                workspace_diags: SeverityBuckets::default(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
                workspace_diags: SeverityBuckets::default(),
                cursor_position: CursorPosition { row: 42, col: 7 },
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: SeverityBuckets::default(),
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
            pretty_assertions::assert_eq!(statusline.draw(), "%#StatusLine#foo %m %r%=%#StatusLine# 42:8");
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
            statusline.draw(),
            format!(
                "%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1 %#StatusLine#foo %m %r%=%#StatusLine# 42:8",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
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
            statusline.draw(),
            format!(
                "%#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1%#StatusLine# 42:8",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
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
                .collect(), // unchanged (multi-element)
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            format!(
                "%#DiagnosticStatusLineWarn#{}:2 %#DiagnosticStatusLineHint#{}:3 %#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#{}:3 %#DiagnosticStatusLineInfo#{}:1%#StatusLine# 42:8",
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Hint,
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Info
            ),
        );
    }

    #[test]
    fn statusline_draw_when_buffer_diagnostics_inserted_unordered_orders_by_severity() {
        // Insert in non-canonical order (Hint before Warn) and ensure output orders by severity (Warn then Hint).
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(DiagnosticSeverity::Hint, 5), (DiagnosticSeverity::Warn, 1)]
                .into_iter()
                .collect(), // multi-element unchanged
            workspace_diags: SeverityBuckets::default(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            format!(
                "%#DiagnosticStatusLineWarn#{}:1 %#DiagnosticStatusLineHint#{}:5 %#StatusLine#foo %m %r%=%#StatusLine# 42:8",
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Hint
            ),
        );
    }

    #[test]
    fn draw_diagnostics_when_zero_count_returns_empty_string() {
        // Any severity with zero count should yield empty string.
        pretty_assertions::assert_eq!(draw_diagnostics((DiagnosticSeverity::Error, 0)), String::new());
        pretty_assertions::assert_eq!(draw_diagnostics((DiagnosticSeverity::Warn, 0)), String::new());
        pretty_assertions::assert_eq!(draw_diagnostics((DiagnosticSeverity::Info, 0)), String::new());
        pretty_assertions::assert_eq!(draw_diagnostics((DiagnosticSeverity::Hint, 0)), String::new());
        // NOTE: Other is not explicitly tested elsewhere here.
        pretty_assertions::assert_eq!(draw_diagnostics((DiagnosticSeverity::Other, 0)), String::new());
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
        // Affirm draw output matches severity ordering; equality macro takes (actual, expected).
        pretty_assertions::assert_eq!(
            statusline.draw(),
            format!(
                "%#DiagnosticStatusLineError#{}:4 %#DiagnosticStatusLineWarn#{}:3 %#DiagnosticStatusLineInfo#{}:2 %#DiagnosticStatusLineHint#{}:1 %#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#{}:8 %#DiagnosticStatusLineWarn#{}:7 %#DiagnosticStatusLineInfo#{}:6 %#DiagnosticStatusLineHint#{}:5%#StatusLine# 42:8",
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Info,
                DiagnosticSeverity::Hint,
                DiagnosticSeverity::Error,
                DiagnosticSeverity::Warn,
                DiagnosticSeverity::Info,
                DiagnosticSeverity::Hint
            ),
        );
    }

    #[test]
    fn statusline_draw_when_cursor_column_zero_renders_one_based_column() {
        // Column zero (internal 0-based) must render as 1 (human-facing).
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: SeverityBuckets::default(),
            workspace_diags: SeverityBuckets::default(),
            cursor_position: CursorPosition { row: 10, col: 0 },
        };
        pretty_assertions::assert_eq!(statusline.draw(), "%#StatusLine#foo %m %r%=%#StatusLine# 10:1");
    }

    #[test]
    fn statusline_draw_when_cursor_column_non_zero_renders_column_plus_one() {
        // Non-zero column must render raw + 1.
        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: SeverityBuckets::default(),
            workspace_diags: SeverityBuckets::default(),
            cursor_position: CursorPosition { row: 10, col: 5 },
        };
        pretty_assertions::assert_eq!(statusline.draw(), "%#StatusLine#foo %m %r%=%#StatusLine# 10:6");
    }
}
