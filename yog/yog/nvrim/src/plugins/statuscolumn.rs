//! Statuscolumn drawing helpers for buffer-local indicators.

use core::fmt::Display;

use nvim_oxi::Dictionary;
use nvim_oxi::api::Buffer;
use serde::Deserialize;
use ytil_noxi::buffer::BufferExt;

use crate::diagnostics::DiagnosticSeverity;

/// Markup for a visible space in the Nvim statuscolumn.
/// Plain spaces (" ") are not rendered; they must be wrapped in highlight markup like `%#Normal# %*`.
const EMPTY_SPACE: &str = "%#Normal# %*";

/// [`Dictionary`] exposing statuscolumn draw helpers.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
    }
}

/// Draws the status column for the current buffer.
fn draw((cur_lnum, extmarks, opts): (String, Vec<Extmark>, Option<Opts>)) -> Option<String> {
    let current_buffer = Buffer::current();
    let buf_type = current_buffer.get_buf_type()?;

    Some(draw_statuscolumn(
        &buf_type,
        &cur_lnum,
        extmarks.into_iter().filter_map(Extmark::into_meta),
        opts,
    ))
}

/// Constructs the status column string for the current line.
fn draw_statuscolumn(
    current_buffer_type: &str,
    cur_lnum: &str,
    metas: impl Iterator<Item = ExtmarkMeta>,
    opts: Option<Opts>,
) -> String {
    if current_buffer_type == "grug-far" || current_buffer_type == "terminal" {
        return String::new();
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
    if let Some(git_extmark) = git_extmark {
        git_extmark.write(&mut out);
    } else {
        out.push_str(EMPTY_SPACE);
    }
    if let Some(highest_severity_diag) = highest_severity_diag {
        highest_severity_diag.meta.write(&mut out);
    } else {
        out.push_str(EMPTY_SPACE);
    }
    if opts.is_some_and(|o| o.show_line_numbers) {
        out.push(' ');
        out.push_str("%=% ");
        out.push_str(cur_lnum);
        out.push(' ');
    }
    out
}

/// Configuration options for the status column.
#[derive(Deserialize)]
struct Opts {
    show_line_numbers: bool,
}

ytil_noxi::impl_nvim_deserializable!(Opts);

/// Internal selection of the highest ranked diagnostic extmark.
#[cfg_attr(test, derive(Debug))]
struct SelectedDiag {
    rank: u8,
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

ytil_noxi::impl_nvim_deserializable!(Extmark);

/// Metadata associated with an extmark.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(Debug))]
struct ExtmarkMeta {
    sign_hl_group: SignHlGroup,
    sign_text: Option<String>,
}

impl ExtmarkMeta {
    /// Writes the formatted extmark metadata into `out`.
    fn write(&self, out: &mut String) {
        let displayed_symbol: &str = match self.sign_hl_group {
            SignHlGroup::DiagnosticError => DiagnosticSeverity::Error.symbol(),
            SignHlGroup::DiagnosticWarn => DiagnosticSeverity::Warn.symbol(),
            SignHlGroup::DiagnosticInfo => DiagnosticSeverity::Info.symbol(),
            SignHlGroup::DiagnosticHint => DiagnosticSeverity::Hint.symbol(),
            SignHlGroup::DiagnosticOk | SignHlGroup::Git(_) | SignHlGroup::Other(_) => {
                self.sign_text.as_ref().map_or("", |x| x.trim())
            }
        };
        // %#<HlGroup>#<text>%*
        out.push('%');
        out.push('#');
        out.push_str(self.sign_hl_group.as_str());
        out.push('#');
        out.push_str(displayed_symbol);
        out.push('%');
        out.push('*');
    }
}

/// Enumerates known and dynamic highlight groups for status column signs.
#[derive(Clone, Debug, Eq, PartialEq)]
enum SignHlGroup {
    DiagnosticError,
    DiagnosticWarn,
    DiagnosticInfo,
    DiagnosticHint,
    DiagnosticOk,
    Git(String),
    Other(String),
}

impl SignHlGroup {
    /// Returns the canonical string form used by Nvim for this group.
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
        // Use if/else instead of match to move the already-owned `s` into Git/Other variants,
        // avoiding a redundant `.to_string()` allocation on every non-diagnostic extmark.
        Ok(if s == "DiagnosticSignError" {
            Self::DiagnosticError
        } else if s == "DiagnosticSignWarn" {
            Self::DiagnosticWarn
        } else if s == "DiagnosticSignInfo" {
            Self::DiagnosticInfo
        } else if s == "DiagnosticSignHint" {
            Self::DiagnosticHint
        } else if s == "DiagnosticSignOk" {
            Self::DiagnosticOk
        } else if s.contains("GitSigns") {
            Self::Git(s)
        } else {
            Self::Other(s)
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
        pretty_assertions::assert_eq!(out, format!("{EMPTY_SPACE}{EMPTY_SPACE} %=% 42 "));
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
        pretty_assertions::assert_eq!(out, format!("{EMPTY_SPACE}%#DiagnosticSignError#x%* %=% 42 "));
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
        pretty_assertions::assert_eq!(out, format!("%#GitSignsFoo#|%*{EMPTY_SPACE} %=% 42 "));
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
        pretty_assertions::assert_eq!(out, "%#GitSignsFoo#|%*%#DiagnosticSignError#x%* %=% 42 ");
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
        pretty_assertions::assert_eq!(out, "");
    }

    #[rstest]
    #[case(None)]
    #[case(Some(Opts { show_line_numbers: false }))]
    fn draw_statuscolumn_when_line_numbers_disabled_returns_no_line_numbers(#[case] opts: Option<Opts>) {
        let out = draw_statuscolumn("foo", "42", std::iter::empty(), opts);
        pretty_assertions::assert_eq!(out, format!("{EMPTY_SPACE}{EMPTY_SPACE}"));
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
        pretty_assertions::assert_eq!(out, "%#GitSignsFoo#|%*%#DiagnosticSignError#x%*");
    }

    fn mk_extmark_meta(group: SignHlGroup, text: &str) -> ExtmarkMeta {
        ExtmarkMeta {
            sign_hl_group: group,
            sign_text: Some(text.to_string()),
        }
    }
}
