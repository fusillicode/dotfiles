//! Extension macros and helpers for bridging Rust and Nvim (`nvim_oxi`).
//!
//! Defines `dict!` for ergonomic [`nvim_oxi::Dictionary`] construction plus `fn_from!` to wrap Rust
//! functions into Nvim callable `Function` objects.

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

/// Implements [`nvim_oxi::conversion::FromObject`] and [`nvim_oxi::lua::Poppable`]
/// for a type that derives [`serde::Deserialize`].
///
/// Eliminates the repeated boilerplate of deserializing Lua objects via `nvim_oxi::serde::Deserializer`.
#[macro_export]
macro_rules! impl_nvim_deserializable {
    ($ty:ty) => {
        impl ::nvim_oxi::conversion::FromObject for $ty {
            fn from_object(obj: ::nvim_oxi::Object) -> ::std::result::Result<Self, ::nvim_oxi::conversion::Error> {
                <Self as ::serde::Deserialize>::deserialize(::nvim_oxi::serde::Deserializer::new(obj))
                    .map_err(::std::convert::Into::into)
            }
        }

        impl ::nvim_oxi::lua::Poppable for $ty {
            unsafe fn pop(
                lstate: *mut ::nvim_oxi::lua::ffi::State,
            ) -> ::std::result::Result<Self, ::nvim_oxi::lua::Error> {
                // SAFETY: The caller (nvim-oxi framework) guarantees that:
                // 1. `lstate` is a valid pointer to an initialized Lua state
                // 2. The Lua stack has at least one value to pop
                unsafe {
                    let obj = ::nvim_oxi::Object::pop(lstate)?;
                    <Self as ::nvim_oxi::conversion::FromObject>::from_object(obj)
                        .map_err(::nvim_oxi::lua::Error::pop_error_from_err::<Self, _>)
                }
            }
        }
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

    use crate::dict::DictionaryExt as _;

    #[test]
    fn dict_macro_empty_creates_empty_dictionary() {
        let actual = dict!();
        assert_eq!(actual.len(), 0);
    }

    #[test]
    fn dict_macro_creates_a_dictionary_with_basic_key_value_pairs() {
        let actual = dict! { "foo": 1, bar: "baz", "num": 3_i64 };
        let expected = Dictionary::from_iter([
            ("bar", Object::from("baz")),
            ("foo", Object::from(1)),
            ("num", Object::from(3_i64)),
        ]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn dict_macro_creates_nested_dictionaries() {
        let k = String::from("alpha");
        let inner = dict! { inner_key: "value" };
        let actual = dict! { (k): 10_i64, "beta": inner.clone() };
        let expected = Dictionary::from_iter([("alpha", Object::from(10_i64)), ("beta", Object::from(inner))]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn dictionary_ext_get_t_works_as_expected() {
        let dict = dict! { "foo": "42" };
        assert2::let_assert!(Err(err) = dict.get_t::<nvim_oxi::String>("bar"));
        assert_eq!(err.format_current_context().to_string(), "missing dict value");
        assert_eq!(dict.get_t::<nvim_oxi::String>("foo").unwrap(), "42");

        let dict = dict! { "foo": 42 };
        assert2::let_assert!(Err(err) = dict.get_t::<nvim_oxi::String>("foo"));
        assert_eq!(err.format_current_context().to_string(), "unexpected object kind");
    }

    #[test]
    fn dictionary_ext_get_dict_works_as_expected() {
        let dict = dict! { "foo": "42" };
        assert_eq!(dict.get_dict(&["bar"]).unwrap(), None);

        let dict = dict! { "foo": 42 };
        assert2::let_assert!(Err(err) = dict.get_dict(&["foo"]));
        assert_eq!(err.format_current_context().to_string(), "unexpected object kind");

        let expected = dict! { "bar": "42" };
        let dict = dict! { "foo": expected.clone() };
        assert_eq!(dict.get_dict(&["foo"]).unwrap(), Some(expected));
    }
}
