//! Statusline drawing helpers with diagnostics and Git change aggregation.

use std::cell::RefCell;
use std::fmt::Write;
use std::path::Path;
use std::path::PathBuf;

use nvim_oxi::Array;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use serde::Deserialize;
use strum::IntoEnumIterator;
use ytil_noxi::buffer::BufferExt;
use ytil_noxi::buffer::CursorPosition;

use crate::diagnostics::DiagnosticSeverity;

const DRAW_TRIGGERS: &[&str] = &[
    "BufEnter",
    "BufFilePost",
    "BufWritePost",
    "CursorMoved",
    "DiagnosticChanged",
    "DirChanged",
    "FocusGained",
    "ShellCmdPost",
    "VimResume",
];
const GIT_ADDED_HIGHLIGHT: &str = "Added";
const GIT_REMOVED_HIGHLIGHT: &str = "Removed";

/// Diagnostic emitted by Nvim for statusline aggregation.
#[derive(Deserialize)]
pub struct Diagnostic {
    /// The buffer number.
    bufnr: i32,
    /// The severity of the diagnostic.
    severity: DiagnosticSeverity,
}

ytil_noxi::impl_nvim_deserializable!(Diagnostic);

/// [`Dictionary`] exposing statusline draw helpers.
///
/// Note: `draw_triggers` creates a new Object each call. This cannot be cached in a static
/// because [`nvim_oxi::Object`] is tied to the Neovim Lua state (not Sync) and unavailable at
/// static initialization. Since [`dict()`] is called once at plugin init, the overhead is minimal.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
        "invalidate_git_stats": fn_from!(invalidate_git_stats),
        "draw_triggers": DRAW_TRIGGERS.iter().map(ToString::to_string).collect::<Object>()
    }
}

thread_local! {
    /// Cached `(buffer_handle, relative_path)` to avoid recomputing the buffer path on every
    /// `CursorMoved` event. Automatically invalidated when the active buffer handle changes
    /// (e.g. on `BufEnter`).
    static CACHED_BUFFER_PATH: RefCell<Option<(i32, Option<String>)>> = const { RefCell::new(None) };

    /// Cached Git statistics for the active buffer or current working directory.
    static CACHED_GIT_STATS: RefCell<Option<CachedGitStats>> = const { RefCell::new(None) };
}

/// Added and removed tracked lines for one status-line scope.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GitLineStats {
    added: usize,
    removed: usize,
}

impl GitLineStats {
    /// Adds one file's Git statistics to this aggregate.
    const fn add_file(&mut self, file_stats: &ytil_git::diff::FileDiffStats) {
        self.added = self.added.saturating_add(file_stats.added);
        self.removed = self.removed.saturating_add(file_stats.removed);
    }

    /// Returns whether this scope has no line changes to display.
    const fn is_empty(self) -> bool {
        self.added == 0 && self.removed == 0
    }

    /// Writes the colored `+N` and `-N` metrics to a status-line string.
    fn write_to(self, target: &mut String, prepend_space: bool) {
        if self.is_empty() {
            return;
        }

        if self.added > 0 {
            if prepend_space {
                target.push(' ');
            }
            let _ = write!(target, "%#{GIT_ADDED_HIGHLIGHT}#+{}", self.added);
        } else if prepend_space {
            target.push(' ');
        }

        if self.removed > 0 {
            if self.added > 0 {
                target.push(' ');
            }
            let _ = write!(target, "%#{GIT_REMOVED_HIGHLIGHT}#-{}", self.removed);
        }
    }
}

/// Git line statistics for the whole repository and the active buffer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GitStats {
    workspace: GitLineStats,
    current_buffer: GitLineStats,
}

impl GitStats {
    /// Aggregates repository statistics and selects the active buffer's statistics when present.
    fn from_file_stats<I>(file_stats: I, current_buffer_path: Option<&Path>) -> Self
    where
        I: IntoIterator<Item = ytil_git::diff::FileDiffStats>,
    {
        let mut stats = Self::default();
        for file_stats in file_stats {
            stats.workspace.add_file(&file_stats);
            if current_buffer_path.is_some_and(|path| file_stats.path == path) {
                stats.current_buffer.add_file(&file_stats);
            }
        }
        stats
    }
}

/// Cached Git statistics keyed by the active buffer and repository discovery path.
#[derive(Debug)]
struct CachedGitStats {
    buffer_nr: i32,
    discovery_path: PathBuf,
    stats: GitStats,
}

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

/// Represents the status line with buffer path, diagnostics, and Git statistics.
#[derive(Debug)]
struct Statusline<'a> {
    current_buffer_path: Option<&'a str>,
    current_buffer_diags: SeverityBuckets,
    workspace_diags: SeverityBuckets,
    git_stats: GitStats,
    cursor_position: Option<CursorPosition>,
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

        // Build final statusline in a single pre-allocated buffer to avoid intermediate
        // format! allocation and the current_buffer_path_segment temporary String.
        let estimated_len = workspace_diags_segment
            .len()
            .saturating_add(current_buffer_diags_segment.len())
            .saturating_add(self.current_buffer_path.map_or(0, str::len))
            .saturating_add(40);
        let mut out = String::with_capacity(estimated_len);
        out.push_str(&current_buffer_diags_segment);
        self.git_stats.current_buffer.write_to(&mut out, false);
        let _ = write!(out, "%#StatusLine# ");
        if let Some(buf_path) = self.current_buffer_path {
            let _ = write!(out, "{buf_path} ");
        }
        if let Some(ref pos) = self.cursor_position {
            let _ = write!(out, "{}:{} ", pos.row, pos.adjusted_col());
        }
        out.push_str(&workspace_diags_segment);
        self.git_stats
            .workspace
            .write_to(&mut out, !workspace_diags_segment.is_empty());
        let _ = write!(out, "%#StatusLine#");
        out
    }
}

/// Invalidates cached Git statistics so the next draw reads the repository again.
fn invalidate_git_stats(_: ()) {
    CACHED_GIT_STATS.with(|cache| *cache.borrow_mut() = None);
}

/// Retrieves Git statistics for the active buffer, or the current working directory when unnamed.
fn get_git_stats(current_buffer: &Buffer, current_buffer_nr: i32) -> GitStats {
    let current_buffer_path = ytil_noxi::buffer::get_absolute_path(Some(current_buffer));
    let Some(discovery_path) = current_buffer_path.clone().or_else(get_current_working_directory) else {
        return GitStats::default();
    };

    let cached_stats = CACHED_GIT_STATS.with(|cache| {
        let cache = cache.borrow();
        cache
            .as_ref()
            .filter(|cached| cached.buffer_nr == current_buffer_nr && cached.discovery_path == discovery_path)
            .map(|cached| cached.stats)
    });
    if let Some(cached_stats) = cached_stats {
        return cached_stats;
    }

    let Ok(repo) = ytil_git::repo::discover(&discovery_path) else {
        return GitStats::default();
    };
    let repo_root = ytil_git::repo::get_root(&repo);
    let Ok(relative_buffer_path) = current_buffer_path
        .as_deref()
        .map(|path| path.strip_prefix(&repo_root).map(Path::to_path_buf))
        .transpose()
    else {
        return GitStats::default();
    };

    let stats = ytil_git::diff::get_line_stats(&repo_root).map_or_else(
        |_| GitStats::default(),
        |file_stats| GitStats::from_file_stats(file_stats, relative_buffer_path.as_deref()),
    );
    CACHED_GIT_STATS.with(|cache| {
        *cache.borrow_mut() = Some(CachedGitStats {
            buffer_nr: current_buffer_nr,
            discovery_path,
            stats,
        });
    });
    stats
}

/// Retrieves Neovim's current working directory for Git discovery without a named buffer.
fn get_current_working_directory() -> Option<PathBuf> {
    nvim_oxi::api::call_function::<_, String>("getcwd", Array::new())
        .ok()
        .map(PathBuf::from)
}

/// Draws the status line with diagnostic and Git change information.
fn draw(diagnostics: Vec<Diagnostic>) -> String {
    let current_buffer = nvim_oxi::api::get_current_buf();
    let current_buffer_nr = current_buffer.handle();

    // Return `%#Normal#` instead of empty string in case of terminal buffers
    // to blend the statusline with the editor background even when a statusline
    // background color is set.
    if current_buffer.is_terminal() {
        return "%#Normal#".to_string();
    }

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

    let cursor_position = CursorPosition::get_current();
    let git_stats = get_git_stats(&current_buffer, current_buffer_nr);

    let mut statusline = Statusline {
        current_buffer_path: current_buffer_path.as_deref(),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: SeverityBuckets::default(),
        git_stats,
        cursor_position,
    };
    for diagnostic in diagnostics {
        statusline.workspace_diags.inc(diagnostic.severity);
        if current_buffer_nr == diagnostic.bufnr {
            statusline.current_buffer_diags.inc(diagnostic.severity);
        }
    }

    statusline.draw()
}

/// Writes the diagnostic count directly to the target string, avoiding intermediate allocation.
fn write_diagnostics(target: &mut String, severity: DiagnosticSeverity, diags_count: u16) {
    if diags_count == 0 {
        return;
    }
    let (hg_group_dyn_part, severity_label) = match severity {
        DiagnosticSeverity::Error => ("Error", "E"),
        DiagnosticSeverity::Warn => ("Warn", "W"),
        DiagnosticSeverity::Info => ("Info", "I"),
        DiagnosticSeverity::Hint | DiagnosticSeverity::Other => ("Hint", "H"),
    };
    // write! to String is infallible, so we can safely ignore the result
    let _ = write!(
        target,
        "%#DiagnosticStatusLine{hg_group_dyn_part}#{severity_label}:{diags_count}"
    );
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
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::default_diags(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: SeverityBuckets::default(),
        git_stats: GitStats::default(),
        cursor_position: Some(CursorPosition { row: 42, col: 7 }),
    })]
    #[case::buffer_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        workspace_diags: SeverityBuckets::default(),
        git_stats: GitStats::default(),
        cursor_position: Some(CursorPosition { row: 42, col: 7 }),
    })]
    #[case::workspace_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: SeverityBuckets::default(),
        workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        git_stats: GitStats::default(),
        cursor_position: Some(CursorPosition { row: 42, col: 7 }),
    })]
    #[case::both_zero(Statusline {
        current_buffer_path: Some("foo"),
        current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
        git_stats: GitStats::default(),
        cursor_position: Some(CursorPosition { row: 42, col: 7 }),
    })]
    fn test_statusline_draw_when_all_diagnostics_absent_or_zero_renders_plain_statusline(
        #[case] statusline: Statusline,
    ) {
        assert_that!(statusline.draw(), eq("%#StatusLine# foo 42:8 %#StatusLine#"));
    }

    #[test]
    fn test_statusline_draw_when_current_buffer_has_diagnostics_renders_buffer_group_before_path() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            workspace_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };
        assert_that!(
            statusline.draw(),
            eq("%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1 %#StatusLine# foo 42:8 %#StatusLine#")
        );
    }

    #[test]
    fn test_statusline_draw_when_workspace_has_diagnostics_renders_workspace_group_after_path() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 0)).collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(),
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };
        assert_that!(
            statusline.draw(),
            eq("%#StatusLine# foo 42:8 %#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1%#StatusLine#")
        );
    }

    #[test]
    fn test_statusline_draw_when_both_buffer_and_workspace_have_diagnostics_renders_separate_groups() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Hint, 3), (DiagnosticSeverity::Warn, 2)]
                .into_iter()
                .collect(),
            workspace_diags: [(DiagnosticSeverity::Info, 1), (DiagnosticSeverity::Error, 3)]
                .into_iter()
                .collect(), // unchanged (multi-element)
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };
        assert_that!(
            statusline.draw(),
            eq(
                "%#DiagnosticStatusLineWarn#W:2 %#DiagnosticStatusLineHint#H:3 %#StatusLine# foo 42:8 %#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1%#StatusLine#"
            )
        );
    }

    #[test]
    fn test_statusline_draw_when_git_stats_exist_renders_separate_buffer_and_workspace_groups() {
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: std::iter::once((DiagnosticSeverity::Info, 1)).collect(),
            workspace_diags: std::iter::once((DiagnosticSeverity::Error, 2)).collect(),
            git_stats: GitStats {
                workspace: GitLineStats { added: 12, removed: 4 },
                current_buffer: GitLineStats { added: 3, removed: 1 },
            },
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };

        assert_that!(
            statusline.draw(),
            eq(
                "%#DiagnosticStatusLineInfo#I:1 %#Added#+3 %#Removed#-1%#StatusLine# foo 42:8 %#DiagnosticStatusLineError#E:2 %#Added#+12 %#Removed#-4%#StatusLine#"
            )
        );
    }

    #[rstest]
    #[case::added_only(GitLineStats { added: 3, removed: 0 }, "%#Added#+3")]
    #[case::removed_only(GitLineStats { added: 0, removed: 2 }, "%#Removed#-2")]
    #[case::no_changes(GitLineStats::default(), "")]
    fn test_git_line_stats_write_to_when_counts_vary_renders_expected_metrics(
        #[case] stats: GitLineStats,
        #[case] expected: &str,
    ) {
        let mut output = String::new();
        stats.write_to(&mut output, false);
        assert_that!(output, eq(expected));
    }

    #[test]
    fn test_git_stats_from_file_stats_when_multiple_files_exist_aggregates_workspace_and_selects_buffer() {
        let stats = GitStats::from_file_stats(
            vec![
                ytil_git::diff::FileDiffStats {
                    path: "src/main.rs".into(),
                    added: 3,
                    removed: 1,
                },
                ytil_git::diff::FileDiffStats {
                    path: "src/lib.rs".into(),
                    added: 5,
                    removed: 2,
                },
            ],
            Some(Path::new("src/main.rs")),
        );

        assert_eq!(
            stats,
            GitStats {
                workspace: GitLineStats { added: 8, removed: 3 },
                current_buffer: GitLineStats { added: 3, removed: 1 },
            }
        );
    }

    #[test]
    fn test_git_stats_from_file_stats_when_buffer_path_is_missing_keeps_only_workspace_stats() {
        let stats = GitStats::from_file_stats(
            vec![ytil_git::diff::FileDiffStats {
                path: "src/main.rs".into(),
                added: 3,
                removed: 1,
            }],
            None,
        );

        assert_eq!(
            stats,
            GitStats {
                workspace: GitLineStats { added: 3, removed: 1 },
                current_buffer: GitLineStats::default(),
            }
        );
    }

    #[test]
    fn test_statusline_draw_when_buffer_diagnostics_inserted_unordered_orders_by_severity() {
        // Insert in non-canonical order (Hint before Warn) and ensure output orders by severity (Warn then Hint).
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: [(DiagnosticSeverity::Hint, 5), (DiagnosticSeverity::Warn, 1)]
                .into_iter()
                .collect(), // multi-element unchanged
            workspace_diags: SeverityBuckets::default(),
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };
        assert_that!(
            statusline.draw(),
            eq("%#DiagnosticStatusLineWarn#W:1 %#DiagnosticStatusLineHint#H:5 %#StatusLine# foo 42:8 %#StatusLine#")
        );
    }

    #[rstest]
    #[case::error(DiagnosticSeverity::Error)]
    #[case::warn(DiagnosticSeverity::Warn)]
    #[case::info(DiagnosticSeverity::Info)]
    #[case::hint(DiagnosticSeverity::Hint)]
    #[case::other(DiagnosticSeverity::Other)]
    fn test_draw_diagnostics_when_zero_count_returns_empty_string(#[case] severity: DiagnosticSeverity) {
        // Any severity with zero count should yield empty string.
        assert_that!(draw_diagnostics((severity, 0)), eq(String::new()));
    }

    #[test]
    fn test_statusline_draw_when_all_severity_counts_present_orders_buffer_and_workspace_diagnostics_by_severity() {
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
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 42, col: 7 }),
        };
        // Affirm draw output matches severity ordering; equality macro takes (actual, expected).
        assert_that!(
            statusline.draw(),
            eq(
                "%#DiagnosticStatusLineError#E:4 %#DiagnosticStatusLineWarn#W:3 %#DiagnosticStatusLineInfo#I:2 %#DiagnosticStatusLineHint#H:1 %#StatusLine# foo 42:8 %#DiagnosticStatusLineError#E:8 %#DiagnosticStatusLineWarn#W:7 %#DiagnosticStatusLineInfo#I:6 %#DiagnosticStatusLineHint#H:5%#StatusLine#"
            )
        );
    }

    #[test]
    fn test_statusline_draw_when_no_path_and_no_cursor_renders_only_highlight_groups() {
        // When both path and cursor position are absent, only the highlight groups remain.
        let statusline = Statusline {
            current_buffer_path: None,
            current_buffer_diags: SeverityBuckets::default(),
            workspace_diags: SeverityBuckets::default(),
            git_stats: GitStats::default(),
            cursor_position: None,
        };
        assert_that!(statusline.draw(), eq("%#StatusLine# %#StatusLine#"));
    }

    #[rstest]
    #[case::zero_column(0, "%#StatusLine# foo 10:1 %#StatusLine#")]
    #[case::non_zero_column(5, "%#StatusLine# foo 10:6 %#StatusLine#")]
    fn test_statusline_draw_when_cursor_column_renders_correctly(#[case] col: usize, #[case] expected: &str) {
        // Column zero (internal 0-based) must render as 1 (human-facing).
        // Non-zero column must render raw + 1.
        let statusline = Statusline {
            current_buffer_path: Some("foo"),
            current_buffer_diags: SeverityBuckets::default(),
            workspace_diags: SeverityBuckets::default(),
            git_stats: GitStats::default(),
            cursor_position: Some(CursorPosition { row: 10, col }),
        };
        assert_that!(statusline.draw(), eq(expected));
    }
}
