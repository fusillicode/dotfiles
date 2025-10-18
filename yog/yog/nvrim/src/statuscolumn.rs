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
    let cur_buf_type = nvim_oxi::api::get_option_value::<String>("buftype", &opts)
        .inspect_err(|error| {
            crate::oxi_ext::api::notify_error(&format!(
                "cannot get buftype of current buffer | buffer={cur_buf:#?} error={error:#?}"
            ));
        })
        .ok()?;

    let mut extmark_meta: Vec<_> = extmarks.iter().filter_map(|extmark| extmark.meta().cloned()).collect();
    for meta in &mut extmark_meta {
        meta.override_sign_text();
    }

    Some(Statuscolumn::draw(&cur_buf_type, cur_lnum, extmark_meta))
}

/// Represents an extmark in Nvim.
#[derive(Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
pub struct Extmark(u32, usize, usize, Option<ExtmarkMeta>);

impl Extmark {
    /// Returns the [`ExtmarkMeta`] of the extmark if present.
    pub const fn meta(&self) -> Option<&ExtmarkMeta> {
        self.3.as_ref()
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
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

/// Metadata associated with an extmark.
#[derive(Clone, Deserialize)]
pub struct ExtmarkMeta {
    /// The highlight group for the sign.
    sign_hl_group: SignHlGroup,
    /// The text of the sign, optional due to grug-far buffers.
    sign_text: Option<String>,
}

impl ExtmarkMeta {
    /// Draws the extmark metadata as a formatted string.
    fn draw(&self) -> String {
        format!(
            "%#{}#{}%*",
            self.sign_hl_group,
            self.sign_text.as_ref().map_or("", |x| x.trim())
        )
    }

    /// Overrides diagnostic sign text with severity shorthand.
    ///
    /// Replaces the current [`ExtmarkMeta::sign_text`] with a canonical
    /// single-letter representation derived from the [`DiagnosticSeverity`]
    /// associated to the diagnostic variants of [`SignHlGroup`]. Non-diagnostic
    /// variants (ok / git / other) keep their existing text unchanged.
    ///
    /// # Rationale
    /// Normalizing diagnostic sign text ensures consistent rendering regardless
    /// of what upstream plugins put into the extmark.
    pub fn override_sign_text(&mut self) {
        let new_sign_text = match self.sign_hl_group {
            SignHlGroup::DiagnosticError => Some(DiagnosticSeverity::Error.to_string()),
            SignHlGroup::DiagnosticWarn => Some(DiagnosticSeverity::Warn.to_string()),
            SignHlGroup::DiagnosticInfo => Some(DiagnosticSeverity::Info.to_string()),
            SignHlGroup::DiagnosticHint => Some(DiagnosticSeverity::Hint.to_string()),
            SignHlGroup::DiagnosticOk | SignHlGroup::Git(_) | SignHlGroup::Other(_) => self.sign_text.clone(),
        };
        self.sign_text = new_sign_text;
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

/// Represents the status column with various signs and line number.
#[derive(Default)]
struct Statuscolumn {
    /// Current line number.
    cur_lnum: String,
    /// Error diagnostic sign.
    error: Option<ExtmarkMeta>,
    /// Git sign.
    git: Option<ExtmarkMeta>,
    /// Hint diagnostic sign.
    hint: Option<ExtmarkMeta>,
    /// Info diagnostic sign.
    info: Option<ExtmarkMeta>,
    /// Ok diagnostic sign.
    ok: Option<ExtmarkMeta>,
    /// Warning diagnostic sign.
    warn: Option<ExtmarkMeta>,
}

impl Statuscolumn {
    /// Draws the status column based on buffer type and [`ExtmarkMeta`].
    ///
    /// Special case:
    /// - Returns a single space (`" "`) when `cur_buf_type == "grug-far"` to avoid clutter in grug-far search buffers.
    ///
    /// # Rationale
    /// Rendering a minimal placeholder for special buffer types keeps alignment predictable without introducing
    /// diagnostic / git sign artifacts that are irrelevant in those contexts.
    fn draw(cur_buf_type: &str, cur_lnum: String, extmarks: Vec<ExtmarkMeta>) -> String {
        match cur_buf_type {
            "grug-far" => " ".into(),
            _ => Self::new(cur_lnum, extmarks).to_string(),
        }
    }

    /// Creates a new [`Statuscolumn`] from line number and [`ExtmarkMeta`].
    fn new(cur_lnum: String, extmarks: Vec<ExtmarkMeta>) -> Self {
        let mut statuscolumn = Self {
            cur_lnum,
            ..Default::default()
        };

        for extmark in extmarks {
            match extmark.sign_hl_group.as_str() {
                "DiagnosticSignError" => statuscolumn.error = Some(extmark),
                "DiagnosticSignWarn" => statuscolumn.warn = Some(extmark),
                "DiagnosticSignInfo" => statuscolumn.info = Some(extmark),
                "DiagnosticSignHint" => statuscolumn.hint = Some(extmark),
                "DiagnosticSignOk" => statuscolumn.ok = Some(extmark),
                git if git.contains("GitSigns") => statuscolumn.git = Some(extmark),
                _ => (),
            }
        }

        statuscolumn
    }
}

/// Implementation of [`core::fmt::Display`] for [`Statuscolumn`].
impl core::fmt::Display for Statuscolumn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let diag_sign = [&self.error, &self.warn, &self.info, &self.hint, &self.ok]
            .iter()
            .find_map(|s| s.as_ref().map(ExtmarkMeta::draw))
            .unwrap_or_else(|| " ".into());

        let git_sign = self.git.as_ref().map_or_else(|| " ".into(), ExtmarkMeta::draw);

        write!(f, "{}{}%=% {} ", diag_sign, git_sign, self.cur_lnum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuscolumn_draw_works_as_expected() {
        // No extmarks
        let out = Statuscolumn::draw("foo", "42".into(), vec![]);
        assert_eq!("  %=% 42 ", &out);

        // 1 diagnostic sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![ExtmarkMeta {
                sign_hl_group: SignHlGroup::DiagnosticError,
                sign_text: Some("E".into()),
            }],
        );
        assert_eq!("%#DiagnosticSignError#E%* %=% 42 ", &out);

        // Multiple diagnostics extmarks and only the higher severity sign is displayed
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![
                ExtmarkMeta {
                    sign_hl_group: SignHlGroup::DiagnosticError,
                    sign_text: Some("E".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: SignHlGroup::DiagnosticWarn,
                    sign_text: Some("W".into()),
                },
            ],
        );
        assert_eq!("%#DiagnosticSignError#E%* %=% 42 ", &out);

        // git sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![ExtmarkMeta {
                sign_hl_group: SignHlGroup::Git("GitSignsFoo".into()),
                sign_text: Some("|".into()),
            }],
        );
        assert_eq!(" %#GitSignsFoo#|%*%=% 42 ", &out);

        // Multiple diagnostics extmarks and a git sign
        let out = Statuscolumn::draw(
            "foo",
            "42".into(),
            vec![
                ExtmarkMeta {
                    sign_hl_group: SignHlGroup::DiagnosticError,
                    sign_text: Some("E".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: SignHlGroup::DiagnosticWarn,
                    sign_text: Some("W".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: SignHlGroup::Git("GitSignsFoo".into()),
                    sign_text: Some("|".into()),
                },
            ],
        );
        assert_eq!("%#DiagnosticSignError#E%*%#GitSignsFoo#|%*%=% 42 ", &out);
    }
}
