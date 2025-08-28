use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;

/// Trait for extracting typed values from Neovim objects.
pub trait OxiExtract {
    type Out;

    /// Extracts a typed value from a Neovim Object with error context.
    fn extract(obj: &Object, key: &str, dict: &Dictionary) -> color_eyre::Result<Self::Out>;
}

/// Implementation for extracting String values from Neovim objects.
impl OxiExtract for nvim_oxi::String {
    type Out = String;

    fn extract(obj: &Object, key: &str, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(obj.clone())
            .with_context(|| unexpected_kind_error_msg(obj, key, dict, ObjectKind::String))?;
        Ok(out.to_string())
    }
}

/// Implementation for extracting i64 values from Neovim objects.
impl OxiExtract for nvim_oxi::Integer {
    type Out = i64;

    fn extract(obj: &Object, key: &str, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(obj.clone())
            .with_context(|| unexpected_kind_error_msg(obj, key, dict, ObjectKind::Integer))?;
        Ok(out)
    }
}

/// Extension trait for [Dictionary] to provide typed getters.
pub trait DictionaryExt {
    /// Gets a typed value from the dictionary using the OxiExtract trait.
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out>;

    /// Gets a nested [Dictionary] from the [Dictionary] by a sequence of keys.
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;
}

/// Implementation of DictionaryExt for Dictionary providing typed getters.
impl DictionaryExt for Dictionary {
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out> {
        let obj = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;
        T::extract(obj, key, self)
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

/// Generates an error message for unexpected [Object] kind.
pub fn unexpected_kind_error_msg(obj: &Object, key: &str, dict: &Dictionary, expected_kind: ObjectKind) -> String {
    format!(
        "value {obj:#?} of key {key:?} in dict {dict:#?} is {0:#?} but {expected_kind:?} was expected",
        obj.kind()
    )
}

/// Creates an error for missing value in [Dictionary].
pub fn no_value_matching(query: &[&str], dict: &Dictionary) -> color_eyre::eyre::Error {
    eyre!("no value matching query {query:?} in dict {dict:#?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_ext_get_t_works_as_expected() {
        let dict = Dictionary::from_iter([("foo", "42")]);
        assert_eq!(
            r#"no value matching query ["bar"] in dict { foo: "42" }"#,
            dict.get_t::<nvim_oxi::String>("bar").unwrap_err().to_string()
        );

        let dict = Dictionary::from_iter([("foo", 42)]);
        assert_eq!(
            r#"value 42 of key "foo" in dict { foo: 42 } is Integer but String was expected"#,
            dict.get_t::<nvim_oxi::String>("foo").unwrap_err().to_string()
        );

        let dict = Dictionary::from_iter([("foo", "42")]);
        assert_eq!("42", dict.get_t::<nvim_oxi::String>("foo").unwrap());
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
