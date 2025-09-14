use color_eyre::eyre::Context;
use nvim_oxi::Dictionary;
use nvim_oxi::Object;
use nvim_oxi::ObjectKind;

/// Trait for extracting typed values from Nvim objects.
pub trait OxiExtract {
    type Out;

    /// Extracts a typed value from an Nvim [Object] by key from a [`Dictionary`] with error context.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An underlying operation fails.
    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out>;
}

/// Implementation for extracting [String] values from Nvim objects.
impl OxiExtract for nvim_oxi::String {
    type Out = String;

    /// Extract from dict.
    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(value.clone())
            .with_context(|| unexpected_kind_error_msg(value, key, dict, ObjectKind::String))?;
        Ok(out.to_string())
    }
}

/// Implementation for extracting i64 values from Nvim objects.
impl OxiExtract for nvim_oxi::Integer {
    type Out = Self;

    /// Extract from dict.
    fn extract_from_dict(key: &str, value: &Object, dict: &Dictionary) -> color_eyre::Result<Self::Out> {
        let out = Self::try_from(value.clone())
            .with_context(|| unexpected_kind_error_msg(value, key, dict, ObjectKind::Integer))?;
        Ok(out)
    }
}

/// Generates an error message for unexpected [Object] kind.
pub fn unexpected_kind_error_msg(obj: &Object, key: &str, dict: &Dictionary, expected_kind: ObjectKind) -> String {
    format!(
        "value {obj:#?} of key {key:?} in dict {dict:#?} is {0:#?} but {expected_kind:?} was expected",
        obj.kind()
    )
}
