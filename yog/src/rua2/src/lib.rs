use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;
use nvim_oxi::api::types::LogLevel;

use crate::cli_flags::CliFlags;

mod cli_flags;
mod diagnostics;
mod fkr;
mod statuscolumn;
mod statusline;
mod test_runner;

#[nvim_oxi::plugin]
fn rua2() -> Dictionary {
    Dictionary::from_iter([
        ("format_diagnostic", diagnostics::formatter::format()),
        ("sort_diagnostics", diagnostics::sorter::sort()),
        ("filter_diagnostics", diagnostics::filter::filter()),
        ("draw_statusline", statusline::draw()),
        ("draw_statuscolumn", statuscolumn::draw()),
        ("create_fkr_cmds", fkr::create_cmds()),
        ("get_fd_cli_flags", cli_flags::fd::FdCliFlags.get()),
        ("get_rg_cli_flags", cli_flags::rg::RgCliFlags.get()),
        ("run_test", test_runner::run_test()),
    ])
}

pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &Default::default()) {
        nvim_oxi::dbg!(format!("can't notify error {msg:?}, error {error:#?}"));
    }
}

pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &Default::default()) {
        nvim_oxi::dbg!(format!("can't notify warning {msg:?}, error {error:#?}"));
    }
}

#[allow(dead_code)]
trait DictionaryExt {
    fn get_string(&self, key: &str) -> color_eyre::Result<String>;
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;
}

impl DictionaryExt for Dictionary {
    fn get_string(&self, key: &str) -> color_eyre::Result<String> {
        let obj = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;

        let out = nvim_oxi::String::try_from(obj.clone())
            .with_context(|| unexpected_kind_error_msg(obj, key, self, ObjectKind::String))?;

        Ok(out.to_string())
    }

    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>> {
        let mut current = self.clone();

        for key in keys {
            let Some(obj) = current.get(key) else { return Ok(None) };
            current = Dictionary::try_from(obj.clone())
                .with_context(|| unexpected_kind_error_msg(obj, key, &current, ObjectKind::Dictionary))?;
        }

        Ok(Some(current.clone()))
    }
}

#[allow(dead_code)]
fn unexpected_kind_error_msg(obj: &Object, key: &str, dict: &Dictionary, expected_kind: ObjectKind) -> String {
    format!(
        "value {obj:#?} of key {key:?} in dict {dict:#?} is {0:#?} but {expected_kind:?} was expected",
        obj.kind()
    )
}

pub fn no_value_matching(query: &[&str], dict: &Dictionary) -> color_eyre::eyre::Error {
    eyre!("no value matching query {query:?} in dict {dict:#?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_ext_get_string_works_as_expected() {
        let dict = Dictionary::from_iter([("foo", "42")]);
        assert_eq!(
            r#"no value matching query ["bar"] in dict { foo: "42" }"#,
            dict.get_string("bar").unwrap_err().to_string()
        );

        let dict = Dictionary::from_iter([("foo", 42)]);
        assert_eq!(
            r#"value 42 of key "foo" in dict { foo: 42 } is Integer but String was expected"#,
            dict.get_string("foo").unwrap_err().to_string()
        );

        let dict = Dictionary::from_iter([("foo", "42")]);
        assert_eq!("42", dict.get_string("foo").unwrap());
    }

    #[test]
    fn test_dictionary_ext_get_dict_works_as_expected() {
        let dict = Dictionary::from_iter([("foo", "42")]);
        assert_eq!(None, dict.get_dict(&["bar"]).unwrap());

        let dict = Dictionary::from_iter([("foo", 42)]);
        assert_eq!(
            r#"value 42 of key "foo" in dict { foo: 42 } is Integer but Dictionary was expected"#,
            dict.get_dict(&["foo"]).unwrap_err().to_string()
        );

        let expected = Dictionary::from_iter([("bar", "42")]);
        let dict = Dictionary::from_iter([("foo", expected.clone())]);
        assert_eq!(Some(expected), dict.get_dict(&["foo"]).unwrap());
    }
}
