//! Statuscolumn drawing helpers for buffer-local indicators.
//!
//! Supplies `statuscolumn.dict()` exposing `draw`, rendering line numbers / extmarks while honoring
//! special buffer types (e.g. minimal output for transient search buffers). Errors are notified via
//! [`ytil_nvim_oxi::api::notify_error`].

use core::fmt::Display;

use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;
use ytil_nvim_oxi::buffer::BufferExt;

use crate::diagnostics::DiagnosticSeverity;

/// [`Dictionary`] exposing statuscolumn draw helpers.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
    }
}

/// Draws the status column for the current buffer.
///
/// # Returns
/// - `Some(String)`: formatted status column when the current buffer `buftype` is successfully retrieved.
/// - `None`: if `buftype` retrieval fails (details logged via [`ytil_nvim_oxi::api::notify_error`]).
///
/// Special cases:
/// - When `buftype == "grug-far"` returns a single space string to minimize visual noise in transient search buffers.
///
/// # Rationale
/// Using `Option<String>` (instead of empty placeholder) allows caller-side distinction between an intentional blank
/// status column (special buffer type) and an error acquiring required state.
fn draw((cur_lnum, extmarks, opts): (String, Vec<Extmark>, Option<Opts>)) -> Option<String> {
    let cur_buf = Buffer::current();
    let buf_type = cur_buf
        .get_buf_type()
        .inspect_err(|error| {
            ytil_nvim_oxi::api::notify_error(format!(
                "cannot get buftype of current buffer | buffer={cur_buf:#?} error={error:#?}"
            ));
        })
        .ok()?;

    Some(draw_statuscolumn(
        &buf_type,
        &cur_lnum,
        extmarks.into_iter().filter_map(Extmark::into_meta),
        opts,
    ))
}

/// Constructs the status column string for the current line.
///
/// # Arguments
/// - `cur_buf_type` Current buffer `buftype` (used for special-case elision like `"grug-far"`).
/// - `cur_lnum` Current line number string (already formatted by the caller / Vim script).
/// - `metas` Iterator of extmark metadata for the current line; only items with a present [`ExtmarkMeta`] are yielded.
///
/// # Returns
/// - Formatted status column string containing (at most) one diagnostic sign (highest severity), one Git sign (first
///   encountered), a spacer + line number with surrounding separators.
///
/// # Assumptions
/// - `metas` yields at most a small number of items (typical per-line sign density is low).
/// - Caller has already restricted extmarks to those relevant for the line being drawn.
///
/// # Rationale
/// - Single pass selects highest severity and first git sign to avoid repeated scans.
/// - Early break once an Error (rank 5) and a Git sign are both determined prevents unnecessary iteration.
/// - Manual string building reduces intermediate allocation versus collecting sign fragments.
///
/// # Performance
/// - Allocates once with a conservative capacity heuristic (`lnum.len() + 64`).
/// - O(n) over `metas`, short-circuiting when optimal state reached.
/// - Rank computation is a simple match with small constant cost.
fn draw_statuscolumn(
    cur_buf_type: &str,
    cur_lnum: &str,
    metas: impl Iterator<Item = ExtmarkMeta>,
    opts: Option<Opts>,
) -> String {
    if cur_buf_type == "grug-far" {
        return " ".into();
    }

    let mut highest_severity_diag: Option<SelectedDiag> = None;
    let mut git_extmark: Option<ExtmarkMeta> = None;

    for meta in metas {
        match meta.sign_hl_group {
            SignHlGroup::DiagnosticError
            | SignHlGroup::DiagnosticWarn
            | SignHlGroup::DiagnosticInfo
            | SignHlGroup::DiagnosticHint
            | SignHlGroup::DiagnosticOk => {
                let rank = meta.sign_hl_group.rank();
                match &highest_severity_diag {
                    Some(sel) if sel.rank >= rank => {}
                    _ => highest_severity_diag = Some(SelectedDiag { rank, meta }),
                }
            }
            SignHlGroup::Git(_) if git_extmark.is_none() => git_extmark = Some(meta),
            SignHlGroup::Git(_) | SignHlGroup::Other(_) => {}
        }
        // Early break: if we already have top severity (Error rank 5) and have determined git presence
        // (either captured or impossible to capture later because we already saw a git sign or caller provided none).
        if let Some(sel) = &highest_severity_diag
            && sel.rank == 5
            && git_extmark.is_some()
        {
            break;
        }
    }

    // Capacity heuristic: each sign ~ 32 chars + lnum + static separators.
    let mut out = String::with_capacity(cur_lnum.len().saturating_add(64));
    if let Some(highest_severity_diag) = highest_severity_diag {
        highest_severity_diag.meta.write(&mut out);
    } else {
        out.push(' ');
    }
    if let Some(git_extmark) = git_extmark {
        git_extmark.write(&mut out);
    } else {
        out.push(' ');
    }
    if opts.is_some_and(|o| o.show_line_numbers) {
        out.push_str("%=% ");
        out.push_str(cur_lnum);
        out.push(' ');
    }
    out
}

#[derive(Deserialize)]
struct Opts {
    show_line_numbers: bool,
}

/// Implementation of [`FromObject`] for [`Extmark`].
impl FromObject for Opts {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`Poppable`] for [`Opts`].
impl Poppable for Opts {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        // SAFETY: Delegates to nvim_oxi object popping then deserializes.
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Internal selection of the highest ranked diagnostic extmark.
///
/// Captures both the numeric rank (see [`SignHlGroup::rank`]) and the associated
/// [`ExtmarkMeta`] to allow deferred rendering after the scan completes.
#[cfg_attr(test, derive(Debug))]
struct SelectedDiag {
    /// Severity rank (higher means more severe); non-diagnostic signs use 0.
    rank: u8,
    /// The metadata of the chosen diagnostic sign.
    meta: ExtmarkMeta,
}

/// Represents an extmark in Nvim.
#[derive(Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
struct Extmark(u32, usize, usize, Option<ExtmarkMeta>);

impl Extmark {
    /// Consumes the extmark returning its metadata (if any).
    fn into_meta(self) -> Option<ExtmarkMeta> {
        self.3
    }
}

/// Implementation of [`FromObject`] for [`Extmark`].
impl FromObject for Extmark {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

/// Implementation of [`Poppable`] for [`Extmark`].
impl Poppable for Extmark {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        // SAFETY: Delegates to nvim_oxi object popping then deserializes.
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Metadata associated with an extmark.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(Debug))]
struct ExtmarkMeta {
    /// The highlight group for the sign.
    sign_hl_group: SignHlGroup,
    /// The text of the sign, optional due to grug-far buffers.
    sign_text: Option<String>,
}

impl ExtmarkMeta {
    /// Writes the formatted extmark metadata into `out`.
    ///
    /// - Performs inline normalization for diagnostic variants (except `Ok`), mapping them to canonical severity glyphs
    ///   from [`DiagnosticSeverity::glyph`].
    /// - Leaves `Ok` / Git / Other variants using their existing trimmed `sign_text` (empty placeholder when absent).
    ///
    /// # Rationale
    /// Appending directly avoids per-sign allocation of an intermediate [`String`].
    fn write(&self, out: &mut String) {
        let displayed_glyph: &str = match self.sign_hl_group {
            SignHlGroup::DiagnosticError => DiagnosticSeverity::Error.glyph(),
            SignHlGroup::DiagnosticWarn => DiagnosticSeverity::Warn.glyph(),
            SignHlGroup::DiagnosticInfo => DiagnosticSeverity::Info.glyph(),
            SignHlGroup::DiagnosticHint => DiagnosticSeverity::Hint.glyph(),
            SignHlGroup::DiagnosticOk | SignHlGroup::Git(_) | SignHlGroup::Other(_) => {
                self.sign_text.as_ref().map_or("", |x| x.trim())
            }
        };
        // %#<HlGroup>#<text>%*
        out.push('%');
        out.push('#');
        out.push_str(self.sign_hl_group.as_str());
        out.push('#');
        out.push_str(displayed_glyph);
        out.push('%');
        out.push('*');
    }
}

/// Enumerates known and dynamic highlight groups for status column signs.
///
/// - Provides explicit variants for the standard diagnostic signs.
/// - Captures Git related signs (`GitSigns*`) while retaining their concrete highlight group string in the
///   [`SignHlGroup::Git`] variant.
/// - Any other (custom / plugin) highlight group is retained verbatim in [`SignHlGroup::Other`].
#[derive(Clone, Debug, Eq, PartialEq)]
enum SignHlGroup {
    /// `DiagnosticSignError` highlight group.
    DiagnosticError,
    /// `DiagnosticSignWarn` highlight group.
    DiagnosticWarn,
    /// `DiagnosticSignInfo` highlight group.
    DiagnosticInfo,
    /// `DiagnosticSignHint` highlight group.
    DiagnosticHint,
    /// `DiagnosticSignOk` highlight group.
    DiagnosticOk,
    /// A Git-related sign highlight group (contains `GitSigns`).
    Git(String),
    /// Any other highlight group string not matched above.
    Other(String),
}

impl SignHlGroup {
    /// Returns the canonical string form used by Neovim for this group.
    ///
    /// # Returns
    /// - A static diagnostic string for diagnostic variants.
    /// - The original owned string slice for dynamic variants (`Git`, `Other`).
    const fn as_str(&self) -> &str {
        match self {
            Self::DiagnosticError => "DiagnosticSignError",
            Self::DiagnosticWarn => "DiagnosticSignWarn",
            Self::DiagnosticInfo => "DiagnosticSignInfo",
            Self::DiagnosticHint => "DiagnosticSignHint",
            Self::DiagnosticOk => "DiagnosticSignOk",
            Self::Git(s) | Self::Other(s) => s.as_str(),
        }
    }

    /// Severity ranking used to pick the highest diagnostic.
    ///
    /// # Returns
    /// - Numeric priority (higher means more severe) for diagnostic variants.
    /// - `0` for non-diagnostic variants (Git / Other).
    ///
    /// # Rationale
    /// Encapsulating the rank logic in the enum keeps selection code simpler and
    /// removes the need for a standalone helper.
    #[inline]
    const fn rank(&self) -> u8 {
        match self {
            Self::DiagnosticError => 5,
            Self::DiagnosticWarn => 4,
            Self::DiagnosticInfo => 3,
            Self::DiagnosticHint => 2,
            Self::DiagnosticOk => 1,
            Self::Git(_) | Self::Other(_) => 0,
        }
    }
}

impl Display for SignHlGroup {
    /// Formats the highlight group as the raw group string.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for SignHlGroup {
    /// Deserializes a highlight group string into a typed [`SignHlGroup`].
    ///
    /// # Errors
    /// Never returns an error beyond underlying string deserialization; every
    /// string maps to some variant.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "DiagnosticSignError" => Self::DiagnosticError,
            "DiagnosticSignWarn" => Self::DiagnosticWarn,
            "DiagnosticSignInfo" => Self::DiagnosticInfo,
            "DiagnosticSignHint" => Self::DiagnosticHint,
            "DiagnosticSignOk" => Self::DiagnosticOk,
            git_hl_group if git_hl_group.contains("GitSigns") => Self::Git(git_hl_group.to_string()),
            other_hl_group => Self::Other(other_hl_group.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[test]
    fn draw_statuscolumn_when_no_extmarks_returns_placeholders() {
        let out = draw_statuscolumn(
            "foo",
            "42",
            std::iter::empty(),
            Some(Opts {
                show_line_numbers: true,
            }),
        );
        pretty_assertions::assert_eq!(out, "  %=% 42 ");
    }

    #[test]
    fn draw_statuscolumn_when_diagnostic_error_and_warn_displays_error() {
        let metas = vec![
            mk_extmark_meta(SignHlGroup::DiagnosticError, "E"),
            mk_extmark_meta(SignHlGroup::DiagnosticWarn, "W"),
        ];
        let out = draw_statuscolumn(
            "foo",
            "42",
            metas.into_iter(),
            Some(Opts {
                show_line_numbers: true,
            }),
        );
        // Canonical normalized error sign text is 'x'.
        pretty_assertions::assert_eq!(out, "%#DiagnosticSignError#x%* %=% 42 ");
    }

    #[test]
    fn draw_statuscolumn_when_git_sign_present_displays_git_sign() {
        let metas = vec![mk_extmark_meta(SignHlGroup::Git("GitSignsFoo".into()), "|")];
        let out = draw_statuscolumn(
            "foo",
            "42",
            metas.into_iter(),
            Some(Opts {
                show_line_numbers: true,
            }),
        );
        pretty_assertions::assert_eq!(out, " %#GitSignsFoo#|%*%=% 42 ");
    }

    #[test]
    fn draw_statuscolumn_when_diagnostics_and_git_sign_displays_both() {
        let metas = vec![
            mk_extmark_meta(SignHlGroup::DiagnosticError, "E"),
            mk_extmark_meta(SignHlGroup::DiagnosticWarn, "W"),
            mk_extmark_meta(SignHlGroup::Git("GitSignsFoo".into()), "|"),
        ];
        let out = draw_statuscolumn(
            "foo",
            "42",
            metas.into_iter(),
            Some(Opts {
                show_line_numbers: true,
            }),
        );
        pretty_assertions::assert_eq!(out, "%#DiagnosticSignError#x%*%#GitSignsFoo#|%*%=% 42 ");
    }

    #[test]
    fn draw_statuscolumn_when_grug_far_buffer_returns_single_space() {
        let out = draw_statuscolumn(
            "grug-far",
            "7",
            std::iter::empty(),
            Some(Opts {
                show_line_numbers: true,
            }),
        );
        pretty_assertions::assert_eq!(out, " ");
    }

    #[rstest]
    #[case(None)]
    #[case(Some(Opts { show_line_numbers: false }))]
    fn draw_statuscolumn_when_line_numbers_disabled_returns_no_line_numbers(#[case] opts: Option<Opts>) {
        let out = draw_statuscolumn("foo", "42", std::iter::empty(), opts);
        pretty_assertions::assert_eq!(out, "  ");
    }

    #[rstest]
    #[case(None)]
    #[case(Some(Opts { show_line_numbers: false }))]
    fn draw_statuscolumn_when_line_numbers_disabled_with_extmarks_returns_no_line_numbers(#[case] opts: Option<Opts>) {
        let metas = vec![
            mk_extmark_meta(SignHlGroup::DiagnosticError, "E"),
            mk_extmark_meta(SignHlGroup::Git("GitSignsFoo".into()), "|"),
        ];
        let out = draw_statuscolumn("foo", "42", metas.into_iter(), opts);
        pretty_assertions::assert_eq!(out, "%#DiagnosticSignError#x%*%#GitSignsFoo#|%*");
    }

    fn mk_extmark_meta(group: SignHlGroup, text: &str) -> ExtmarkMeta {
        ExtmarkMeta {
            sign_hl_group: group,
            sign_text: Some(text.to_string()),
        }
    }
}
