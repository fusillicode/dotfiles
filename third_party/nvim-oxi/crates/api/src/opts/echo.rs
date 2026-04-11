/// Options passed to [`echo()`](crate::echo).
#[derive(Clone, Debug, Default, macros::OptsBuilder)]
#[repr(C)]
pub struct EchoOpts {
    #[builder(mask)]
    mask: u64,

    #[builder(argtype = "bool")]
    err: types::Boolean,

    #[builder(argtype = "bool")]
    verbose: types::Boolean,

    #[builder(method = "truncate", argtype = "bool")]
    _truncate: types::Boolean,

    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    kind: types::String,

    #[builder(
        generics = "Id: Into<types::Object>",
        argtype = "Id",
        inline = "{0}.into()"
    )]
    id: types::Object,

    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    title: types::String,

    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    status: types::String,

    #[builder(argtype = "u32", inline = "{0} as types::Integer")]
    percent: types::Integer,

    #[builder(
        generics = "S: Into<types::String>",
        argtype = "S",
        inline = "{0}.into()"
    )]
    source: types::String,

    #[builder(argtype = "types::Dictionary")]
    data: types::Dictionary,
}
