use color_eyre::eyre::Context;
use color_eyre::eyre::eyre;
use nvim_oxi::Dictionary;
use nvim_oxi::ObjectKind;

use crate::oxi_ext::extract::OxiExtract;

/// Extension trait for [`Dictionary`] to provide typed getters.
pub trait DictionaryExt {
    /// Gets a typed value from the dictionary using the [`OxiExtract`] trait.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The key is missing.
    /// - The value cannot be converted to the requested type (unexpected kind).
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out>;

    /// Gets a nested [`Dictionary`] from the [`Dictionary`] by a sequence of keys.
    ///
    /// # Errors
    /// Returns an error if:
    /// - Traversal finds a non-dictionary object for an intermediate key.
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>>;

    fn get_required_dict(&self, keys: &[&str]) -> color_eyre::Result<Dictionary>;
}

/// Implementation of [`DictionaryExt`] for [`Dictionary`] providing typed getters.
impl DictionaryExt for Dictionary {
    /// Get t.
    fn get_t<T: OxiExtract>(&self, key: &str) -> color_eyre::Result<T::Out> {
        let value = self.get(key).ok_or_else(|| no_value_matching(&[key], self))?;
        T::extract_from_dict(key, value, self)
    }

    /// Get dict.
    fn get_dict(&self, keys: &[&str]) -> color_eyre::Result<Option<Dictionary>> {
        let mut current = self.clone();

        for key in keys {
            let Some(obj) = current.get(key) else { return Ok(None) };
            current = Self::try_from(obj.clone()).with_context(|| {
                crate::oxi_ext::extract::unexpected_kind_error_msg(obj, key, &current, ObjectKind::Dictionary)
            })?;
        }

        Ok(Some(current))
    }

    /// Get required dict.
    fn get_required_dict(&self, keys: &[&str]) -> color_eyre::Result<Dictionary> {
        self.get_dict(keys)?.ok_or_else(|| no_value_matching(keys, self))
    }
}

/// Creates an error for missing value in [`Dictionary`].
fn no_value_matching(query: &[&str], dict: &Dictionary) -> color_eyre::eyre::Error {
    eyre!("missing dict value | query={query:#?} dict={dict:#?}")
}
