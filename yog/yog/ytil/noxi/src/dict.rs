//! Typed dictionary extraction helpers for Nvim objects.

use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;
use rootcause::prelude::ResultExt;
use rootcause::report;

use crate::extract::OxiExtract;

/// Extension trait for [`Dictionary`] to provide typed getters.
pub trait DictionaryExt {
    /// Gets a required typed value from the dictionary using the [`OxiExtract`] trait.
    ///
    /// Fails if the key is absent.
    ///
    ///
    /// # Errors
    /// - The key is missing.
    /// - The value exists but cannot be converted to the requested type (unexpected kind).
    fn get_t<T: OxiExtract>(&self, key: &str) -> rootcause::Result<T::Out>;

    /// Gets an optional typed value from the dictionary using the [`OxiExtract`] trait.
    ///
    /// Returns `Ok(None)` if the key is absent instead of treating it as an error.
    ///
    ///
    /// # Errors
    /// - The value exists but cannot be converted to the requested type (unexpected kind).
    fn get_opt_t<T: OxiExtract>(&self, key: &str) -> rootcause::Result<Option<T::Out>>;

    /// Gets an optional nested [`Dictionary`] by traversing a sequence of keys.
    ///
    /// Returns `Ok(None)` if any key in the path is absent.
    ///
    ///
    /// # Errors
    /// - A value is found for an intermediate key but it is not a [`Dictionary`] (unexpected kind).
    fn get_dict(&self, keys: &[&str]) -> rootcause::Result<Option<Dictionary>>;

    /// Gets a required nested [`Dictionary`] by traversing a sequence of keys.
    ///
    /// Fails if any key in the path is missing.
    ///
    ///
    /// # Errors
    /// - A key in the path is missing.
    /// - A value is found for an intermediate key but it is not a [`Dictionary`] (unexpected kind).
    fn get_required_dict(&self, keys: &[&str]) -> rootcause::Result<Dictionary>;
}

/// Implementation of [`DictionaryExt`] for [`Dictionary`] providing typed getters.
impl DictionaryExt for Dictionary {
    fn get_t<T: OxiExtract>(&self, key: &str) -> rootcause::Result<T::Out> {
        let value = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;
        T::extract_from_dict(key, value, self)
    }

    fn get_opt_t<T: OxiExtract>(&self, key: &str) -> rootcause::Result<Option<T::Out>> {
        self.get(key)
            .map(|value| T::extract_from_dict(key, value, self))
            .transpose()
    }

    fn get_dict(&self, keys: &[&str]) -> rootcause::Result<Option<Dictionary>> {
        let mut current = self.clone();

        for key in keys {
            let Some(obj) = current.get(key) else { return Ok(None) };
            current = Self::try_from(obj.clone())
                .context("unexpected object kind")
                .attach_with(|| {
                    crate::extract::unexpected_kind_error_msg(obj, key, &current, ObjectKind::Dictionary)
                })?;
        }

        Ok(Some(current))
    }

    fn get_required_dict(&self, keys: &[&str]) -> rootcause::Result<Dictionary> {
        self.get_dict(keys)?.ok_or_else(|| no_value_matching(keys, self))
    }
}

/// Creates an error for missing value in [`Dictionary`].
fn no_value_matching(query: &[&str], dict: &Dictionary) -> rootcause::Report {
    report!("missing dict value").attach(format!("query={query:#?} dict={dict:#?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict;

    #[test]
    fn get_t_missing_key_errors() {
        let d = dict! { other: 1 };
        assert2::let_assert!(Err(err) = d.get_t::<nvim_oxi::String>("name"));
        assert_eq!(err.format_current_context().to_string(), "missing dict value");
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
        assert_eq!(err.format_current_context().to_string(), "unexpected object kind");
    }

    #[test]
    fn get_required_dict_missing_errors() {
        let d = dict! { root: dict! { leaf: 1 } };
        assert2::let_assert!(Err(err) = d.get_required_dict(&["root", "branch"]));
        assert_eq!(err.format_current_context().to_string(), "missing dict value");
    }
}
