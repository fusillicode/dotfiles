//! Typed dictionary extraction helpers for Nvim objects.
//!
//! Adds [`DictionaryExt`] trait supplying required / optional typed getters and nested dictionary
//! traversal with precise error messages (missing key, unexpected kind).

use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;

use crate::extract::OxiExtract;

/// Extension trait for [`Dictionary`] to provide typed getters.
pub trait DictionaryExt {
    /// Gets a required typed value from the dictionary using the [`OxiExtract`] trait.
    ///
    /// Fails if the key is absent.
    ///
    /// # Returns
    /// - `Ok(T::Out)` when the key exists and the value is successfully extracted.
    ///
    /// # Errors
    /// - The key is missing.
    /// - The value exists but cannot be converted to the requested type (unexpected kind).
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out>;

    /// Gets an optional typed value from the dictionary using the [`OxiExtract`] trait.
    ///
    /// Returns `Ok(None)` if the key is absent instead of treating it as an error.
    ///
    /// # Returns
    /// - `Ok(Some(T::Out))` when the key exists and value is successfully extracted.
    /// - `Ok(None)` when the key does not exist in the [`Dictionary`].
    ///
    /// # Errors
    /// - The value exists but cannot be converted to the requested type (unexpected kind).
    fn get_opt_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<Option<T::Out>>;

    /// Gets an optional nested [`Dictionary`] by traversing a sequence of keys.
    ///
    /// Returns `Ok(None)` if any key in the path is absent.
    ///
    /// # Returns
    /// - `Ok(Some(Dictionary))` when all keys are present and yield a dictionary.
    /// - `Ok(None)` when a key is missing along the path.
    ///
    /// # Errors
    /// - A value is found for an intermediate key but it is not a [`Dictionary`] (unexpected kind).
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;

    /// Gets a required nested [`Dictionary`] by traversing a sequence of keys.
    ///
    /// Fails if any key in the path is missing.
    ///
    /// # Returns
    /// - `Ok(Dictionary)` when all keys are present and yield a dictionary.
    ///
    /// # Errors
    /// - A key in the path is missing.
    /// - A value is found for an intermediate key but it is not a [`Dictionary`] (unexpected kind).
    fn get_required_dict(&self, keys: &[&str]) -> color_eyre::Result<Dictionary>;
}

/// Implementation of [`DictionaryExt`] for [`Dictionary`] providing typed getters.
impl DictionaryExt for Dictionary {
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out> {
        let value = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;
        T::extract_from_dict(key, value, self)
    }

    fn get_opt_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<Option<T::Out>> {
        self.get(key)
            .map(|value| T::extract_from_dict(key, value, self))
            .transpose()
    }

    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>> {
        let mut current = self.clone();

        for key in keys {
            let Some(obj) = current.get(key) else { return Ok(None) };
            current = Self::try_from(obj.clone()).with_context(|| {
                crate::extract::unexpected_kind_error_msg(obj, key, &current, ObjectKind::Dictionary)
            })?;
        }

        Ok(Some(current))
    }

    fn get_required_dict(&self, keys: &[&str]) -> color_eyre::Result<Dictionary> {
        self.get_dict(keys)?.ok_or_else(|| no_value_matching(keys, self))
    }
}

/// Creates an error for missing value in [`Dictionary`].
fn no_value_matching(query: &[&str], dict: &Dictionary) -> color_eyre::eyre::Error {
    eyre!("missing dict value | query={query:#?} dict={dict:#?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict;

    #[test]
    fn get_t_missing_key_errors() {
        let d = dict! { other: 1 };
        assert2::let_assert!(Err(err) = d.get_t::<nvim_oxi::String>("name"));
        let msg = err.to_string();
        assert!(msg.starts_with("missing dict value |"), "actual: {msg}");
        assert!(msg.contains("query=[\n    \"name\",\n]"), "actual: {msg}");
    }

    #[test]
    fn get_opt_t_missing_key_ok_none() {
        let d = dict! { other: 1 };
        assert2::let_assert!(Ok(v) = d.get_opt_t::<nvim_oxi::String>("name"));
        assert!(v.is_none());
    }

    #[test]
    fn get_dict_missing_intermediate_returns_none() {
        let d = dict! { root: dict! { level: dict! { value: 1 } } };
        assert2::let_assert!(Ok(v) = d.get_dict(&["root", "missing", "value"]));
        assert!(v.is_none());
    }

    #[test]
    fn get_dict_intermediate_wrong_type_errors() {
        let d = dict! { root: dict! { leaf: 1 } };
        assert2::let_assert!(Err(err) = d.get_dict(&["root", "leaf", "value"]));
        let msg = err.to_string();
        assert!(msg.contains(" is Integer but Dictionary was expected"), "actual: {msg}");
        assert!(msg.contains("key \"leaf\""), "actual: {msg}");
    }

    #[test]
    fn get_required_dict_missing_errors() {
        let d = dict! { root: dict! { leaf: 1 } };
        assert2::let_assert!(Err(err) = d.get_required_dict(&["root", "branch"]));
        let msg = err.to_string();
        assert!(msg.starts_with("missing dict value |"), "actual: {msg}");
        assert!(
            msg.contains("query=[\n    \"root\",\n    \"branch\",\n]"),
            "actual: {msg}"
        );
    }
}
