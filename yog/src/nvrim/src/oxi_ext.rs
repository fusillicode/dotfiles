use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;
use nvim_oxi::api::Buffer;
use nvim_oxi::api::types::LogLevel;

/// Construct a [`Dictionary`] from key-value pairs, supporting nested [`dict!`] usage.
///
/// Keys can be:
/// - string literals,
/// - identifiers (converted with [`stringify!`]), or
/// - expressions yielding [`String`] or [`&str`].
///
/// Values: any type that implements [`Into<nvim_oxi::Object>`]
#[macro_export]
macro_rules! dict {
    () => {{
        ::nvim_oxi::Dictionary::default()
    }};
    ( $( $key:tt : $value:expr ),+ $(,)? ) => {{
        let mut map: ::std::collections::BTreeMap<
            ::std::borrow::Cow<'static, str>,
            ::nvim_oxi::Object
        > = ::std::collections::BTreeMap::new();
        $(
            let k: ::std::borrow::Cow<'static, str> = $crate::__dict_key_to_cow!($key);
            let v: ::nvim_oxi::Object = ::nvim_oxi::Object::from($value);
            map.insert(k, v);
        )+
        ::nvim_oxi::Dictionary::from_iter(map)
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __dict_key_to_cow {
    ($k:literal) => {
        ::std::borrow::Cow::Borrowed($k)
    };
    ($k:ident) => {
        ::std::borrow::Cow::Borrowed(::std::stringify!($k))
    };
    ($k:expr) => {
        ::std::borrow::Cow::Owned(::std::convert::Into::<::std::string::String>::into($k))
    };
}

/// Turns a Rust function into a [`nvim_oxi::Object`] [`nvim_oxi::Function`].
#[macro_export]
macro_rules! fn_from {
    // Plain function path
    ($path:path) => {
        ::nvim_oxi::Object::from(::nvim_oxi::Function::from_fn($path))
    };
    // Fallback: forward any tokens (supports calls like Type::method(()))
    ($($tokens:tt)+) => {
        ::nvim_oxi::Object::from(::nvim_oxi::Function::from_fn($($tokens)+))
    };
}

/// Trait for extracting typed values from Neovim objects.
pub trait OxiExtract {
    type Out;

    /// Extracts a typed value from a Neovim [Object] by key from a [`Dictionary`] with error context.
    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out>;
}

/// Implementation for extracting [String] values from Neovim objects.
impl OxiExtract for nvim_oxi::String {
    type Out = String;

    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(value.clone())
            .with_context(|| unexpected_kind_error_msg(value, key, dict, ObjectKind::String))?;
        Ok(out.to_string())
    }
}

/// Implementation for extracting i64 values from Neovim objects.
impl OxiExtract for nvim_oxi::Integer {
    type Out = Self;

    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(value.clone())
            .with_context(|| unexpected_kind_error_msg(value, key, dict, ObjectKind::Integer))?;
        Ok(out)
    }
}

/// Extension trait for [`Dictionary`] to provide typed getters.
pub trait DictionaryExt {
    /// Gets a typed value from the dictionary using the [`OxiExtract`] trait.
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out>;

    /// Gets a nested [`Dictionary`] from the [`Dictionary`] by a sequence of keys.
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;
}

/// Implementation of [`DictionaryExt`] for [`Dictionary`] providing typed getters.
impl DictionaryExt for Dictionary {
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out> {
        let value = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;
        T::extract_from_dict(key, value, self)
    }

    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>> {
        let mut current = self.clone();

        for key in keys {
            let Some(obj) = current.get(key) else { return Ok(None) };
            current = Self::try_from(obj.clone())
                .with_context(|| unexpected_kind_error_msg(obj, key, &current, ObjectKind::Dictionary))?;
        }

        Ok(Some(current))
    }
}

/// Extension trait for [`Buffer`] to provide extra functionalities.
pub trait BufferExt {
    /// Fetch a single line from a [`Buffer`] by 0-based index.
    ///
    /// Returns a [`color_eyre::Result`] with the line as [`nvim_oxi::String`].
    /// Errors if the line does not exist at `idx`.
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String>;
}

impl BufferExt for Buffer {
    fn get_line(&self, idx: usize) -> color_eyre::Result<nvim_oxi::String> {
        self.get_lines(idx..=idx, true)?
            .next()
            .ok_or_else(|| eyre!("no line found with idx {idx} for buffer {self:#?}"))
    }
}

/// Generates an error message for unexpected [Object] kind.
pub fn unexpected_kind_error_msg(obj: &Object, key: &str, dict: &Dictionary, expected_kind: ObjectKind) -> String {
    format!(
        "value {obj:#?} of key {key:?} in dict {dict:#?} is {0:#?} but {expected_kind:?} was expected",
        obj.kind()
    )
}

/// Creates an error for missing value in [`Dictionary`].
pub fn no_value_matching(query: &[&str], dict: &Dictionary) -> color_eyre::eyre::Error {
    eyre!("missing value matching query {query:?} in dict {dict:#?}")
}

/// Notifies the user of an error message in Neovim.
pub fn notify_error(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Error, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify error {msg:?}, error {error:#?}"));
    }
}

/// Notifies the user of a warning message in Neovim.
#[expect(dead_code, reason = "Kept for future use")]
pub fn notify_warn(msg: &str) {
    if let Err(error) = nvim_oxi::api::notify(msg, LogLevel::Warn, &dict! {}) {
        nvim_oxi::dbg!(format!("cannot notify warning {msg:?}, error {error:#?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_macro_empty_creates_empty_dictionary() {
        let actual = dict!();
        assert_eq!(0, actual.len());
    }

    #[test]
    fn dict_macro_creates_a_dictionary_with_basic_key_value_pairs() {
        let actual = dict! { "foo": 1, bar: "baz", "num": 3i64 };
        let expected = Dictionary::from_iter([
            ("bar", Object::from("baz")),
            ("foo", Object::from(1)),
            ("num", Object::from(3i64)),
        ]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn dict_macro_creates_nested_dictionaries() {
        let k = String::from("alpha");
        let inner = dict! { inner_key: "value" };
        let actual = dict! { (k): 10i64, "beta": inner.clone() };
        let expected = Dictionary::from_iter([("alpha", Object::from(10i64)), ("beta", Object::from(inner))]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn dictionary_ext_get_t_works_as_expected() {
        let dict = dict! { "foo": "42" };
        assert_eq!(
            r#"missing value matching query ["bar"] in dict { foo: "42" }"#,
            dict.get_t::<nvim_oxi::String>("bar").unwrap_err().to_string()
        );
        assert_eq!("42", dict.get_t::<nvim_oxi::String>("foo").unwrap());

        let dict = dict! { "foo": 42 };
        assert_eq!(
            r#"value 42 of key "foo" in dict { foo: 42 } is Integer but String was expected"#,
            dict.get_t::<nvim_oxi::String>("foo").unwrap_err().to_string()
        );
    }

    #[test]
    fn dictionary_ext_get_dict_works_as_expected() {
        let dict = dict! { "foo": "42" };
        assert_eq!(None, dict.get_dict(&["bar"]).unwrap());

        let dict = dict! { "foo": 42 };
        assert_eq!(
            r#"value 42 of key "foo" in dict { foo: 42 } is Integer but Dictionary was expected"#,
            dict.get_dict(&["foo"]).unwrap_err().to_string()
        );

        let expected = dict! { "bar": "42" };
        let dict = dict! { "foo": expected.clone() };
        assert_eq!(Some(expected), dict.get_dict(&["foo"]).unwrap());
    }
}
