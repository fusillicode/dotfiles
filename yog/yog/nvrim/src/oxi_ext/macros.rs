/// Construct a [`nvim_oxi::Dictionary`] from key-value pairs, supporting nested `dict!` usage.
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
    // Fallback: forward any tokens (supports calls like `Type::method(())`)
    ($($tokens:tt)+) => {
        ::nvim_oxi::Object::from(::nvim_oxi::Function::from_fn($($tokens)+))
    };
}

#[cfg(test)]
mod tests {
    use nvim_oxi::Dictionary;
    use nvim_oxi::Object;

    use crate::oxi_ext::dict::DictionaryExt as _;

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
