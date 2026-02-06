//! Statusline drawing helpers with diagnostics aggregation.

use std::cell::RefCell;
use std::fmt::Write as _;

use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use serde::Deserialize;
use strum::IntoEnumIterator;
use ytil_noxi::buffer::CursorPosition;

use crate::diagnostics::DiagnosticSeverity;

const DRAW_TRIGGERS: &[&str] = &["DiagnosticChanged", "BufEnter", "CursorMoved"];

thread_local! {
    /// Cached `(buffer_handle, relative_path)` to avoid recomputing the buffer path on every
    /// `CursorMoved` event. Automatically invalidated when the active buffer handle changes
    /// (e.g. on `BufEnter`).
    static CACHED_BUFFER_PATH: RefCell<Option<(i32, Option<String>)>> = const { RefCell::new(None) };
}

/// [`Dictionary`] exposing statusline draw helpers.
///
/// Note: `draw_triggers` creates a new Object each call. This cannot be cached in a static
/// because [`nvim_oxi::Object`] is tied to the Neovim Lua state (not Sync) and unavailable at
/// static initialization. Since [`dict()`] is called once at plugin init, the overhead is minimal.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
        "draw_triggers": DRAW_TRIGGERS.iter().map(ToString::to_string).collect::<Object>()
    }
}

/// Draws the status line with diagnostic information.
fn draw(diagnostics: Vec<Diagnostic>) -> Option<String> {
    let current_buffer = nvim_oxi::api::get_current_buf();
    let current_buffer_nr = current_buffer.handle();

    // Use cached buffer path when the buffer handle hasn't changed (avoids FFI + PathBuf work on
    // every CursorMoved). The cache is invalidated implicitly when the handle changes (BufEnter).
    let current_buffer_path = CACHED_BUFFER_PATH.with(|cache| {
        let cached = cache.borrow();
        if let Some((handle, ref path)) = *cached
            && handle == current_buffer_nr
        {
            return path.clone();
        }
        drop(cached);
        let path = ytil_noxi::buffer::get_relative_path_to_cwd(&current_buffer).map(|x| x.display().to_string());
        *cache.borrow_mut() = Some((current_buffer_nr, path.clone()));
        path
    });

    let mut statusline = Statusline {
        current_buffer_path: current_buffer_path.as_deref(),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: SeverityBuckets::default(),
        cursor_position: CursorPosition::get_current()?,
    };
    for diagnostic in diagnostics {
        statusline.workspace_diags.inc(diagnostic.severity);
        if current_buffer_nr == diagnostic.bufnr {
            statusline.current_buffer_diags.inc(diagnostic.severity);
        }
    }

    Some(statusline.draw())
}

/// Diagnostic emitted by Nvim for statusline aggregation.
#[derive(Deserialize)]
pub struct Diagnostic {
    /// The buffer number.
    bufnr: i32,
    /// The severity of the diagnostic.
    severity: DiagnosticSeverity,
}

ytil_noxi::impl_nvim_deserializable!(Diagnostic);

/// Fixed-size aggregation of counts per [`DiagnosticSeverity`].
#[derive(Clone, Copy, Debug, Default)]
struct SeverityBuckets {
    counts: [u16; DiagnosticSeverity::VARIANT_COUNT],
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
    fn get(&self, sev: DiagnosticSeverity) -> u16 {
        let idx = sev as usize;
        self.counts.get(idx).copied().unwrap_or(0)
    }

    /// Iterate over (severity, count) pairs.
    fn iter(&self) -> impl Iterator<Item = (DiagnosticSeverity, u16)> + '_ {
        DiagnosticSeverity::iter().map(|s| (s, self.get(s)))
    }

    /// Approximate rendered length for pre-allocation.
    fn approx_render_len(&self) -> usize {
        let non_zero = self.counts.iter().filter(|&&c| c > 0).count();
        // Each segment roughly: `"%#DiagnosticStatusLineWarn#W:123"` ~ 32 chars worst case; be conservative.
        // Use saturating_mul to satisfy `clippy::arithmetic_side_effects` pedantic lint.
        non_zero.saturating_mul(32)
    }
}

/// Build buckets from iterator of (severity, count).
impl FromIterator<(DiagnosticSeverity, u16)> for SeverityBuckets {
    fn from_iter<T: IntoIterator<Item = (DiagnosticSeverity, u16)>>(iter: T) -> Self {
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
    current_buffer_path: Option<&'a str>,
    current_buffer_diags: SeverityBuckets,
    workspace_diags: SeverityBuckets,
    cursor_position: CursorPosition,
}

impl Statusline<'_> {
    /// Draws the status line as a formatted string.
    fn draw(&self) -> String {
        // Build current buffer diagnostics (with trailing space if any present) manually to avoid
        // iterator allocation and secondary pass (.any()).
        let mut current_buffer_diags_segment = String::with_capacity(self.current_buffer_diags.approx_render_len());
        let mut wrote_any = false;
        for (sev, count) in self.current_buffer_diags.iter() {
            if count == 0 {
                continue;
            }
            if wrote_any {
                current_buffer_diags_segment.push(' ');
            }
            // Write directly to string to avoid intermediate allocation
            write_diagnostics(&mut current_buffer_diags_segment, sev, count);
            wrote_any = true;
        }
        if wrote_any {
            current_buffer_diags_segment.push(' '); // maintain previous trailing space contract
        }

        // Workspace diagnostics (no trailing space).
        let mut workspace_diags_segment = String::with_capacity(self.workspace_diags.approx_render_len());
        let mut first = true;
        for (sev, count) in self.workspace_diags.iter() {
            if count == 0 {
                continue;
            }
            if !first {
                workspace_diags_segment.push(' ');
            }
            // Write directly to string to avoid intermediate allocation
            write_diagnostics(&mut workspace_diags_segment, sev, count);
            first = false;
        }

        let current_buffer_path_segment = self
            .current_buffer_path
            .map(|buf_path| format!("{buf_path} "))
            .unwrap_or_default();

        format!(
            "{workspace_diags_segment}%#StatusLine# {current_buffer_path_segment}{}:{} {current_buffer_diags_segment}%#StatusLine#",
            self.cursor_position.row,
            self.cursor_position.adjusted_col()
        )
    }
}

/// Writes the diagnostic count directly to the target string, avoiding intermediate allocation.
fn write_diagnostics(target: &mut String, severity: DiagnosticSeverity, diags_count: u16) {
    if diags_count == 0 {
        return;
    }
    let hg_group_dyn_part = match severity {
        DiagnosticSeverity::Error => "Error",
        DiagnosticSeverity::Warn => "Warn",
        DiagnosticSeverity::Info => "Info",
        DiagnosticSeverity::Hint | DiagnosticSeverity::Other => "Hint",
    };
    // write! to String is infallible, so we can safely ignore the result
    let _ = write!(target, "%#DiagnosticStatusLine{hg_group_dyn_part}#{diags_count}");
}

/// Draws the diagnostic count for a (severity, count) pair.
/// Kept for test compatibility.
#[cfg(test)]
fn draw_diagnostics((severity, diags_count): (DiagnosticSeverity, u16)) -> String {
    let mut out = String::new();
    write_diagnostics(&mut out, severity, diags_count);
    out
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::default_diags(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: SeverityBuckets::default(),
        cursor_position: CursorPosition { row: 42, col: 7 },
    })]
    #[case::buffer_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        workspace_diags: SeverityBuckets::default(),
        cursor_position: CursorPosition { row: 42, col: 7 },
    })]
    #[case::workspace_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        cursor_position: CursorPosition { row: 42, col: 7 },
    })]
    #[case::both_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        cursor_position: CursorPosition { row: 42, col: 7 },
    })]
    fn statusline_draw_when_all_diagnostics_absent_or_zero_renders_plain_statusline(#[case] statusline: Statusline) {
        pretty_assertions::assert_eq!(statusline.draw(), "%#StatusLine# foo 42:8 %#StatusLine#");
    }

    #[test]
    fn statusline_draw_when_current_buffer_has_diagnostics_renders_buffer_prefix() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            "%#StatusLine# foo 42:8 %#DiagnosticStatusLineError#3 %#DiagnosticStatusLineInfo#1 %#StatusLine#",
        );
    }

    #[test]
    fn statusline_draw_when_workspace_has_diagnostics_renders_workspace_suffix() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            "%#DiagnosticStatusLineError#3 %#DiagnosticStatusLineInfo#1%#StatusLine# foo 42:8 %#StatusLine#",
        );
    }

    #[test]
    fn statusline_draw_when_both_buffer_and_workspace_have_diagnostics_renders_both_prefix_and_suffix() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Hint, 3), (DiagnosticSeverity::Warn, 2)]
                .into_iter()
                .collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(), // unchanged (multi-element)
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            "%#DiagnosticStatusLineError#3 %#DiagnosticStatusLineInfo#1%#StatusLine# foo 42:8 %#DiagnosticStatusLineWarn#2 %#DiagnosticStatusLineHint#3 %#StatusLine#",
        );
    }

    #[test]
    fn statusline_draw_when_buffer_diagnostics_inserted_unordered_orders_by_severity() {
        // Insert in non-canonical order (Hint before Warn) and ensure output orders by severity (Warn then Hint).
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Hint, 5), (DiagnosticSeverity::Warn, 1)]
                .into_iter()
                .collect(), // multi-element unchanged
            workspace_diags: SeverityBuckets::default(),
            cursor_position: CursorPosition { row: 42, col: 7 },
        };
        pretty_assertions::assert_eq!(
            statusline.draw(),
            "%#StatusLine# foo 42:8 %#DiagnosticStatusLineWarn#1 %#DiagnosticStatusLineHint#5 %#StatusLine#",
        );
    }

    #[rstest]
    #[case::error(DiagnosticSeverity::Error)]
    #[case::warn(DiagnosticSeverity::Warn)]
    #[case::info(DiagnosticSeverity::Info)]
    #[case::hint(DiagnosticSeverity::Hint)]
    #[case::other(DiagnosticSeverity::Other)]
    fn draw_diagnostics_when_zero_count_returns_empty_string(#[case] severity: DiagnosticSeverity) {
        // Any severity with zero count should yield empty string.
        pretty_assertions::assert_eq!(draw_diagnostics((severity, 0)), String::new());
    }

    #[test]
    fn statusline_draw_when_all_severity_counts_present_orders_buffer_and_workspace_diagnostics_by_severity() {
        // Insert diagnostics in deliberately scrambled order to validate deterministic ordering.
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [
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
            "%#DiagnosticStatusLineError#8 %#DiagnosticStatusLineWarn#7 %#DiagnosticStatusLineInfo#6 %#DiagnosticStatusLineHint#5%#StatusLine# foo 42:8 %#DiagnosticStatusLineError#4 %#DiagnosticStatusLineWarn#3 %#DiagnosticStatusLineInfo#2 %#DiagnosticStatusLineHint#1 %#StatusLine#",
        );
    }

    #[rstest]
    #[case::zero_column(0, "%#StatusLine# foo 10:1 %#StatusLine#")]
    #[case::non_zero_column(5, "%#StatusLine# foo 10:6 %#StatusLine#")]
    fn statusline_draw_when_cursor_column_renders_correctly(#[case] col: usize, #[case] expected: &str) {
        // Column zero (internal 0-based) must render as 1 (human-facing).
        // Non-zero column must render raw + 1.
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: SeverityBuckets::default(),
            workspace_diags: SeverityBuckets::default(),
            cursor_position: CursorPosition { row: 10, col },
        };
        pretty_assertions::assert_eq!(statusline.draw(), expected);
    }
}
