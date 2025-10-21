//! Statuscolumn drawing helpers for buffer-local indicators.
//!
//! Supplies `statuscolumn.dict()` exposing `draw`, rendering line numbers / extmarks while honoring
//! special buffer types (e.g. minimal output for transient search buffers). Errors are notified via
//! [`crate::oxi_ext::api::notify_error`].

use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;

use crate::diagnostics::DiagnosticSeverity;
use crate::dict;
use crate::fn_from;

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
/// - `None`: if `buftype` retrieval fails (details logged via [`crate::oxi_ext::api::notify_error`]).
///
/// Special cases:
/// - When `buftype == "grug-far"` returns a single space string to minimize visual noise in transient search buffers.
///
/// # Rationale
/// Using `Option<String>` (instead of empty placeholder) allows caller-side distinction between an intentional blank
/// status column (special buffer type) and an error acquiring required state.
fn draw((cur_lnum, extmarks): (String, Vec<Extmark>)) -> Option<String> {
    let cur_buf = Buffer::current();
    let opts = OptionOptsBuilder::default().buf(cur_buf.clone()).build();
    let buf_type = nvim_oxi::api::get_option_value::<String>("buftype", &opts)
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot get buftype of current buffer | buffer={cur_buf:#?} error={error:#?}"
            ));
        })
        .ok()?;

    Some(render_statuscolumn(
        &buf_type,
        &cur_lnum,
        extmarks.into_iter().filter_map(Extmark::into_meta),
    ))
}

/// Represents an extmark in Nvim.
#[derive(Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
pub struct Extmark(u32, usize, usize, Option<ExtmarkMeta>);

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
pub struct ExtmarkMeta {
    /// The highlight group for the sign.
    sign_hl_group: SignHlGroup,
    /// The text of the sign, optional due to grug-far buffers.
    sign_text: Option<String>,
}

impl ExtmarkMeta {
    /// Draws the extmark metadata as a formatted string.
    ///
    /// - Performs inline normalization for diagnostic variants (except `Ok`), mapping them to canonical severity
    ///   letters from [`DiagnosticSeverity`].
    /// - Leaves `Ok` / Git / Other variants using their existing trimmed `sign_text` (empty placeholder when absent).
    ///
    /// # Rationale
    /// Consolidates normalization with rendering so callers never need a
    /// separate pre-processing step.
    fn draw(&self) -> String {
        let shown = match self.sign_hl_group {
            SignHlGroup::DiagnosticError => DiagnosticSeverity::Error.to_string(),
            SignHlGroup::DiagnosticWarn => DiagnosticSeverity::Warn.to_string(),
            SignHlGroup::DiagnosticInfo => DiagnosticSeverity::Info.to_string(),
            SignHlGroup::DiagnosticHint => DiagnosticSeverity::Hint.to_string(),
            SignHlGroup::DiagnosticOk | SignHlGroup::Git(_) | SignHlGroup::Other(_) => {
                self.sign_text.as_ref().map_or("", |x| x.trim()).to_string()
            }
        };
        format!("%#{}#{}%*", self.sign_hl_group, shown)
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
}

impl core::fmt::Display for SignHlGroup {
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

#[cfg_attr(test, derive(Debug))]
struct SelectedDiag {
    rank: u8,
    meta: ExtmarkMeta,
}

fn render_statuscolumn(cur_buf_type: &str, cur_lnum: &str, metas: impl Iterator<Item = ExtmarkMeta>) -> String {
    if cur_buf_type == "grug-far" {
        return " ".into();
    }

    let mut highest_severity: Option<SelectedDiag> = None;
    let mut git: Option<ExtmarkMeta> = None;

    for meta in metas {
        match meta.sign_hl_group {
            SignHlGroup::DiagnosticError
            | SignHlGroup::DiagnosticWarn
            | SignHlGroup::DiagnosticInfo
            | SignHlGroup::DiagnosticHint
            | SignHlGroup::DiagnosticOk => {
                let rank = meta.sign_hl_group.rank();
                match &highest_severity {
                    Some(sel) if sel.rank >= rank => {}
                    _ => highest_severity = Some(SelectedDiag { rank, meta }),
                }
            }
            SignHlGroup::Git(_) if git.is_none() => git = Some(meta),
            SignHlGroup::Git(_) | SignHlGroup::Other(_) => {}
        }
    }

    let diag = highest_severity.map_or_else(|| " ".to_string(), |sel| sel.meta.draw());
    let git = git.map_or_else(|| " ".to_string(), |m| m.draw());

    // Capacity preallocation uses saturating_add to avoid overflow while keeping
    // intent clear under -D clippy::arithmetic-side-effects.
    let cap = diag
        .len()
        .saturating_add(git.len())
        .saturating_add(cur_lnum.len())
        .saturating_add(8);
    let mut out = String::with_capacity(cap);
    out.push_str(&diag);
    out.push_str(&git);
    out.push_str("%=% ");
    out.push_str(cur_lnum);
    out.push(' ');
    out
}

impl SignHlGroup {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_extmark_meta(group: SignHlGroup, text: &str) -> ExtmarkMeta {
        ExtmarkMeta {
            sign_hl_group: group,
            sign_text: Some(text.to_string()),
        }
    }

    #[test]
    fn render_statuscolumn_when_no_extmarks_returns_placeholders() {
        let out = render_statuscolumn("foo", "42", std::iter::empty());
        pretty_assertions::assert_eq!(out, "  %=% 42 ");
    }

    #[test]
    fn render_statuscolumn_when_diagnostic_error_and_warn_displays_error() {
        let metas = vec![
            mk_extmark_meta(SignHlGroup::DiagnosticError, "E"),
            mk_extmark_meta(SignHlGroup::DiagnosticWarn, "W"),
        ];
        let out = render_statuscolumn("foo", "42", metas.into_iter());
        // Canonical normalized error sign text is 'x'.
        pretty_assertions::assert_eq!(out, "%#DiagnosticSignError#x%* %=% 42 ");
    }

    #[test]
    fn render_statuscolumn_when_git_sign_present_displays_git_sign() {
        let metas = vec![mk_extmark_meta(SignHlGroup::Git("GitSignsFoo".into()), "|")];
        let out = render_statuscolumn("foo", "42", metas.into_iter());
        pretty_assertions::assert_eq!(out, " %#GitSignsFoo#|%*%=% 42 ");
    }

    #[test]
    fn render_statuscolumn_when_diagnostics_and_git_sign_displays_both() {
        let metas = vec![
            mk_extmark_meta(SignHlGroup::DiagnosticError, "E"),
            mk_extmark_meta(SignHlGroup::DiagnosticWarn, "W"),
            mk_extmark_meta(SignHlGroup::Git("GitSignsFoo".into()), "|"),
        ];
        let out = render_statuscolumn("foo", "42", metas.into_iter());
        pretty_assertions::assert_eq!(out, "%#DiagnosticSignError#x%*%#GitSignsFoo#|%*%=% 42 ");
    }

    #[test]
    fn render_statuscolumn_when_grug_far_buffer_returns_single_space() {
        let out = render_statuscolumn("grug-far", "7", std::iter::empty());
        pretty_assertions::assert_eq!(out, " ");
    }
}
