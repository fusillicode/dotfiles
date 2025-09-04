use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::Poppable;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;

use crate::dict;
use crate::fn_from;

/// [`Dictionary`] exposing statuscolumn draw helpers.
pub fn dict() -> Dictionary {
    dict! {
        "draw": fn_from!(draw),
    }
}

/// Draws the status column for the current buffer.
fn draw((cur_lnum, extmarks): (String, Vec<Extmark>)) -> Option<String> {
    let cur_buf = Buffer::current();
    let opts = OptionOptsBuilder::default().buf(cur_buf.clone()).build();
    let cur_buf_type = nvim_oxi::api::get_option_value::<String>("buftype", &opts)
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!(
                "cannot get buftype of current buffer #{cur_buf:#?}, error {error:#?}"
            ));
        })
        .ok()?;

    Some(Statuscolumn::draw(
        &cur_buf_type,
        cur_lnum,
        extmarks.iter().filter_map(|extmark| extmark.meta().cloned()).collect(),
    ))
}

/// Represents an extmark in Neovim.
#[derive(Deserialize)]
#[expect(dead_code, reason = "Unused fields are kept for completeness")]
pub struct Extmark(u32, usize, usize, Option<ExtmarkMeta>);

impl Extmark {
    /// Returns the [`ExtmarkMeta`] of the extmark if present.
    pub const fn meta(&self) -> Option<&ExtmarkMeta> {
        self.3.as_ref()
    }
}

/// Metadata associated with an extmark.
#[derive(Deserialize, Clone)]
pub struct ExtmarkMeta {
    /// The highlight group for the sign.
    sign_hl_group: String,
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
    /// Draws the status column based on buffer type and [`ExtmarkMeta`]s.
    fn draw(cur_buf_type: &str, cur_lnum: String, extmarks: Vec<ExtmarkMeta>) -> String {
        match cur_buf_type {
            "grug-far" => " ".into(),
            _ => Self::new(cur_lnum, extmarks).to_string(),
        }
    }

    /// Creates a new [`Statuscolumn`] from line number and [`ExtmarkMeta`]s.
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

/// Implementation of [`Display`] for [`Statuscolumn`].
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
                sign_hl_group: "DiagnosticSignError".into(),
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
                    sign_hl_group: "DiagnosticSignError".into(),
                    sign_text: Some("E".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: "DiagnosticSignWarn".into(),
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
                sign_hl_group: "GitSignsFoo".into(),
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
                    sign_hl_group: "DiagnosticSignError".into(),
                    sign_text: Some("E".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: "DiagnosticSignWarn".into(),
                    sign_text: Some("W".into()),
                },
                ExtmarkMeta {
                    sign_hl_group: "GitSignsFoo".into(),
                    sign_text: Some("|".into()),
                },
            ],
        );
        assert_eq!("%#DiagnosticSignError#E%*%#GitSignsFoo#|%*%=% 42 ", &out);
    }
}
