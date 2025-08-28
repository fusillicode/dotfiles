use std::borrow::Cow;
use std::collections::HashMap;

use nvim_oxi::Array;
use nvim_oxi::Object;
use nvim_oxi::conversion::FromObject;
use nvim_oxi::lua::ffi::State;
use nvim_oxi::serde::Deserializer;
use serde::Deserialize;
use serde_repr::Deserialize_repr;

pub fn draw(diagnostics: Vec<Diagnostic>) -> Option<String> {
    let cur_buf = nvim_oxi::api::get_current_buf();
    let cur_buf_path = cur_buf
        .get_name()
        .inspect_err(|error| {
            crate::oxi_utils::notify_error(&format!(
                "can't get name of current buffer {cur_buf:#?}, error {error:#?}"
            ));
        })
        .ok()?;
    let cwd = nvim_oxi::api::call_function::<Array, String>("getcwd", Array::new())
        .inspect_err(|error| {
            crate::oxi_utils::notify_error(&format!("can't get cwd, error {error:#?}"));
        })
        .ok()?;
    let cur_buf_path = cur_buf_path.to_string_lossy();

    let cur_buf_nr = cur_buf.handle();
    let mut statusline = Statusline {
        cur_buf_path: Cow::Borrowed(cur_buf_path.trim_start_matches(&cwd)),
        cur_buf_diags: HashMap::new(),
        workspace_diags: HashMap::new(),
    };
    for diagnostic in diagnostics {
        if cur_buf_nr == diagnostic.bufnr {
            *statusline.cur_buf_diags.entry(diagnostic.severity).or_insert(0) += 1;
        }
        *statusline.workspace_diags.entry(diagnostic.severity).or_insert(0) += 1;
    }

    Some(statusline.draw())
}

#[derive(Deserialize)]
pub struct Diagnostic {
    bufnr: i32,
    severity: Severity,
}

impl FromObject for Diagnostic {
    fn from_object(obj: Object) -> Result<Self, nvim_oxi::conversion::Error> {
        Self::deserialize(Deserializer::new(obj)).map_err(Into::into)
    }
}

impl nvim_oxi::lua::Poppable for Diagnostic {
    unsafe fn pop(lstate: *mut State) -> Result<Self, nvim_oxi::lua::Error> {
        unsafe {
            let obj = Object::pop(lstate)?;
            Self::from_object(obj).map_err(nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
        }
    }
}

#[derive(Debug, Deserialize_repr, Hash, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum Severity {
    Error = 1,
    Warn = 2,
    Info = 3,
    Hint = 4,
}

impl Severity {
    const ORDER: &'static [Self] = &[Self::Error, Self::Warn, Self::Info, Self::Hint];

    fn draw_diagnostics(&self, diags_count: i32) -> String {
        if diags_count == 0 {
            return "".into();
        }
        let (hg_group, sym) = match self {
            Severity::Error => ("DiagnosticStatusLineError", "E"),
            Severity::Warn => ("DiagnosticStatusLineWarn", "W"),
            Severity::Info => ("DiagnosticStatusLineInfo", "I"),
            Severity::Hint => ("DiagnosticStatusLineHint", "H"),
        };
        format!("%#{hg_group}#{sym}:{diags_count}")
    }
}

#[derive(Debug)]
struct Statusline<'a> {
    cur_buf_path: Cow<'a, str>,
    cur_buf_diags: HashMap<Severity, i32>,
    workspace_diags: HashMap<Severity, i32>,
}

impl<'a> Statusline<'a> {
    fn draw(&self) -> String {
        let mut cur_buf_diags = Severity::ORDER
            .iter()
            .filter_map(|s| self.cur_buf_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        let workspace_diags = Severity::ORDER
            .iter()
            .filter_map(|s| self.workspace_diags.get(s).map(|c| s.draw_diagnostics(*c)))
            .collect::<Vec<_>>()
            .join(" ");

        if !cur_buf_diags.is_empty() {
            cur_buf_diags.push(' ');
        }

        format!(
            "{cur_buf_diags}%#StatusLine#{} %m %r%={workspace_diags}",
            self.cur_buf_path
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_line_draw_works_as_expected() {
        for statusline in [
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: HashMap::new(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
                workspace_diags: HashMap::new(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: HashMap::new(),
                workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
            },
            Statusline {
                cur_buf_path: "foo".into(),
                cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
                workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
            },
        ] {
            let res = statusline.draw();
            assert_eq!(
                "%#StatusLine#foo %m %r%=", &res,
                "unexpected not empty diagnosticts drawn, res {res}, statusline {statusline:#?}"
            );
        }

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Info, 1), (Severity::Error, 3)].into_iter().collect(),
            workspace_diags: [(Severity::Info, 0)].into_iter().collect(),
        };
        assert_eq!(
            "%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1 %#StatusLine#foo %m %r%=",
            &statusline.draw()
        );

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Info, 0)].into_iter().collect(),
            workspace_diags: [(Severity::Info, 1), (Severity::Error, 3)].into_iter().collect(),
        };
        assert_eq!(
            "%#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1",
            &statusline.draw()
        );

        let statusline = Statusline {
            cur_buf_path: "foo".into(),
            cur_buf_diags: [(Severity::Hint, 3), (Severity::Warn, 2)].into_iter().collect(),
            workspace_diags: [(Severity::Info, 1), (Severity::Error, 3)].into_iter().collect(),
        };
        assert_eq!(
            "%#DiagnosticStatusLineWarn#W:2 %#DiagnosticStatusLineHint#H:3 %#StatusLine#foo %m %r%=%#DiagnosticStatusLineError#E:3 %#DiagnosticStatusLineInfo#I:1",
            &statusline.draw()
        );
    }
}
