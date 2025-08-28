use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;

pub trait DictionaryExt {
    fn get_string(&self, key: &str) -> color_eyre::Result<String>;
    fn get_i64(&self, key: &str) -> color_eyre::Result<i64>;
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;
}

impl DictionaryExt for Dictionary {
    fn get_string(&self, key: &str) -> color_eyre::Result<String> {
        let obj = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;

        let out = nvim_oxi::String::try_from(obj.clone())
            .with_context(|| unexpected_kind_error_msg(obj, key, self, ObjectKind::String))?;

        Ok(out.to_string())
    }

    fn get_i64(&self, key: &str) -> color_eyre::Result<i64> {
        let obj = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;

        let out = nvim_oxi::Integer::try_from(obj.clone())
            .with_context(|| unexpected_kind_error_msg(obj, key, self, ObjectKind::Integer))?;

        Ok(out)
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

pub fn unexpected_kind_error_msg(obj: &Object, key: &str, dict: &Dictionary, expected_kind: ObjectKind) -> String {
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
