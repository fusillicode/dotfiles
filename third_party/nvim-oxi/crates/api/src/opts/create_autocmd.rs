use crate::Buffer;
use crate::StringOrInt;
use crate::types::AutocmdCallbackArgs;

pub type ShouldDeleteAutocmd = bool;

/// Options passed to [`create_autocmd()`](crate::create_autocmd).
#[cfg(not(feature = "neovim-nightly"))]
#[derive(Clone, Debug, Default, macros::OptsBuilder)]
#[repr(C)]
pub struct CreateAutocmdOpts {
    #[builder(mask)]
    mask: u64,

    /// A specific `Buffer` for buffer-local autocommands.
    #[builder(argtype = "Buffer", inline = "{0}.0")]
    buffer: types::BufHandle,

    /// Callback to execute when the autocommand is triggered. Cannot be used
    /// together with `command`.
    #[builder(
        generics = r#"F: Into<types::Function<AutocmdCallbackArgs, ShouldDeleteAutocmd>>"#,
        argtype = "F",
        inline = "{0}.into().into()"
    )]
    callback: types::Object,

    /// Vim command to execute when the autocommand is triggered. Cannot be
    /// used together with `callback`.
    // TODO: fix builder(Into).
    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    command: types::String,

    /// Description of the autocommand.
    // TODO: fix builder(Into).
    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    desc: types::String,

    /// The autocommand group name or id to match against.
    #[builder(
        generics = "G: StringOrInt",
        argtype = "G",
        inline = "{0}.to_object()"
    )]
    group: types::Object,

    /// Run nested autocommands.
    #[builder(argtype = "bool")]
    nested: types::Boolean,

    /// Only run the autocommand once.
    #[builder(argtype = "bool")]
    once: types::Boolean,

    /// Patterns to match against.
    #[builder(
        generics = "'a, I: IntoIterator<Item = &'a str>",
        method = "patterns",
        argtype = "I",
        inline = "types::Array::from_iter({0}).into()"
    )]
    pattern: types::Object,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_command_sets_command_field() {
        let opts = CreateAutocmdOpts::builder().command("echo 'hi'").build();
        assert_eq!(opts.command, types::String::from("echo 'hi'"));
    }

    #[cfg(feature = "neovim-nightly")]
    #[test]
    fn builder_buffer_sets_nightly_buf_field() {
        let buffer = Buffer::from(42);
        let buf_handle = buffer.0;
        let opts = CreateAutocmdOpts::builder().buffer(buffer).build();
        assert_eq!(opts.buffer, 0);
        assert_eq!(opts.buf, buf_handle);
    }
}

/// Options passed to [`create_autocmd()`](crate::create_autocmd).
#[cfg(feature = "neovim-nightly")]
#[derive(Clone, Debug, Default, macros::OptsBuilder)]
#[repr(C)]
pub struct CreateAutocmdOpts {
    #[builder(mask)]
    mask: u64,

    /// Deprecated alias kept for nightly ABI compatibility. Use
    /// [`buffer`](CreateAutocmdOptsBuilder::buffer).
    #[builder(skip)]
    buffer: types::BufHandle,

    /// A specific `Buffer` for buffer-local autocommands.
    #[builder(method = "buffer", argtype = "Buffer", inline = "{0}.0")]
    buf: types::BufHandle,

    /// Callback to execute when the autocommand is triggered. Cannot be used
    /// together with `command`.
    #[builder(
        generics = r#"F: Into<types::Function<AutocmdCallbackArgs, ShouldDeleteAutocmd>>"#,
        argtype = "F",
        inline = "{0}.into().into()"
    )]
    callback: types::Object,

    /// Vim command to execute when the autocommand is triggered. Cannot be
    /// used together with `callback`.
    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    command: types::String,

    /// Description of the autocommand.
    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    desc: types::String,

    /// The autocommand group name or id to match against.
    #[builder(
        generics = "G: StringOrInt",
        argtype = "G",
        inline = "{0}.to_object()"
    )]
    group: types::Object,

    /// Run nested autocommands.
    #[builder(argtype = "bool")]
    nested: types::Boolean,

    /// Only run the autocommand once.
    #[builder(argtype = "bool")]
    once: types::Boolean,

    /// Patterns to match against.
    #[builder(
        generics = "'a, I: IntoIterator<Item = &'a str>",
        method = "patterns",
        argtype = "I",
        inline = "types::Array::from_iter({0}).into()"
    )]
    pattern: types::Object,
}
