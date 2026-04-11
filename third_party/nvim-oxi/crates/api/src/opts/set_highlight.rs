use types::Object;
use types::{Boolean, Integer, String as NvimString};

#[cfg(not(feature = "neovim-nightly"))]
#[derive(Clone, Debug, Default, PartialEq, macros::OptsBuilder)]
#[repr(C)]
pub struct SetHighlightOpts {
    #[builder(mask)]
    mask: u64,

    #[builder(argtype = "bool")]
    bold: Boolean,

    #[builder(argtype = "bool")]
    standout: Boolean,

    #[builder(argtype = "bool")]
    strikethrough: Boolean,

    #[builder(argtype = "bool")]
    underline: Boolean,

    #[builder(argtype = "bool")]
    undercurl: Boolean,

    #[builder(argtype = "bool")]
    underdouble: Boolean,

    #[builder(argtype = "bool")]
    underdotted: Boolean,

    #[builder(argtype = "bool")]
    underdashed: Boolean,

    #[builder(argtype = "bool")]
    italic: Boolean,

    #[builder(argtype = "bool")]
    reverse: Boolean,

    #[builder(argtype = "bool")]
    altfont: Boolean,

    #[builder(argtype = "bool")]
    nocombine: Boolean,

    #[builder(method = "builder", argtype = "bool")]
    // The field name is actually `default_`, but I think it somehow gets
    // converted to `default` at build time because the correct mask index
    // is obtained with `default`.
    default: Boolean,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    cterm: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    foreground: Object,

    #[builder(skip)]
    fg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    background: Object,

    #[builder(skip)]
    bg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    ctermfg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    ctermbg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    special: Object,

    #[builder(skip)]
    sp: Object,

    #[cfg(not(feature = "neovim-0-11"))] // Only on 0.10.
    #[builder(
        generics = "Hl: crate::HlGroup",
        argtype = "Hl",
        inline = r#"{ let Ok(hl_id) = {0}.to_hl_id() else { return self; }; hl_id.into() }"#
    )]
    link: Object,

    #[cfg(feature = "neovim-0-11")] // On 0.11 and Nightly.
    #[builder(
        generics = "Hl: crate::HlGroup",
        argtype = "Hl",
        inline = r#"{ let Ok(hl_id) = {0}.to_hl_id() else { return self; }; hl_id }"#
    )]
    link: types::HlGroupId,

    #[cfg(not(feature = "neovim-0-11"))] // Only on 0.10.
    #[builder(skip)]
    global_link: Object,

    #[cfg(feature = "neovim-0-11")] // On 0.11 and Nightly.
    #[builder(skip)]
    global_link: types::HlGroupId,

    #[builder(argtype = "bool")]
    fallback: Boolean,

    #[builder(argtype = "u8", inline = "{0} as Integer")]
    blend: Integer,

    #[builder(argtype = "bool")]
    fg_indexed: Boolean,

    #[builder(argtype = "bool")]
    bg_indexed: Boolean,

    #[builder(argtype = "bool")]
    force: Boolean,

    #[builder(skip)]
    url: NvimString,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "neovim-nightly")]
    #[test]
    fn builder_cterm_accepts_highlight_cterm() {
        let cterm = crate::types::HighlightCterm {
            blink: true,
            bold: true,
            overline: true,
            ..Default::default()
        };
        let opts = SetHighlightOpts::builder().cterm(cterm).build();

        assert_eq!(opts.cterm["bold"], true.into());
        assert_eq!(opts.cterm["blink"], true.into());
        assert_eq!(opts.cterm["overline"], true.into());
    }

    #[cfg(feature = "neovim-nightly")]
    #[test]
    fn builder_sets_nightly_highlight_flags() {
        let opts = SetHighlightOpts::builder()
            .blink(true)
            .conceal(true)
            .dim(true)
            .overline(true)
            .update(true)
            .build();

        assert!(opts.blink);
        assert!(opts.conceal);
        assert!(opts.dim);
        assert!(opts.overline);
        assert!(opts.update);
    }
}

#[cfg(feature = "neovim-nightly")]
#[derive(Clone, Debug, Default, PartialEq, macros::OptsBuilder)]
#[repr(C)]
pub struct SetHighlightOpts {
    #[builder(mask)]
    mask: u64,

    #[builder(argtype = "bool")]
    altfont: Boolean,

    #[builder(argtype = "bool")]
    blink: Boolean,

    #[builder(argtype = "bool")]
    bold: Boolean,

    #[builder(argtype = "bool")]
    conceal: Boolean,

    #[builder(argtype = "bool")]
    dim: Boolean,

    #[builder(argtype = "bool")]
    italic: Boolean,

    #[builder(argtype = "bool")]
    nocombine: Boolean,

    #[builder(argtype = "bool")]
    overline: Boolean,

    #[builder(argtype = "bool")]
    reverse: Boolean,

    #[builder(argtype = "bool")]
    standout: Boolean,

    #[builder(argtype = "bool")]
    strikethrough: Boolean,

    #[builder(argtype = "bool")]
    undercurl: Boolean,

    #[builder(argtype = "bool")]
    underdashed: Boolean,

    #[builder(argtype = "bool")]
    underdotted: Boolean,

    #[builder(argtype = "bool")]
    underdouble: Boolean,

    #[builder(argtype = "bool")]
    underline: Boolean,

    #[builder(method = "builder", argtype = "bool")]
    default: Boolean,

    #[builder(
        generics = "C: Into<types::Dictionary>",
        argtype = "C",
        inline = "{0}.into()"
    )]
    cterm: types::Dictionary,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    foreground: Object,

    #[builder(skip)]
    fg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    background: Object,

    #[builder(skip)]
    bg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    ctermfg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    ctermbg: Object,

    #[builder(argtype = "&str", inline = "types::String::from({0}).into()")]
    special: Object,

    #[builder(skip)]
    sp: Object,

    #[builder(
        generics = "Hl: crate::HlGroup",
        argtype = "Hl",
        inline = r#"{ let Ok(hl_id) = {0}.to_hl_id() else { return self; }; hl_id }"#
    )]
    link: types::HlGroupId,

    #[builder(skip)]
    global_link: types::HlGroupId,

    #[builder(argtype = "bool")]
    fallback: Boolean,

    #[builder(argtype = "u8", inline = "{0} as Integer")]
    blend: Integer,

    #[builder(argtype = "bool")]
    fg_indexed: Boolean,

    #[builder(argtype = "bool")]
    bg_indexed: Boolean,

    #[builder(argtype = "bool")]
    force: Boolean,

    #[builder(argtype = "bool")]
    update: Boolean,

    #[builder(skip)]
    url: NvimString,
}
