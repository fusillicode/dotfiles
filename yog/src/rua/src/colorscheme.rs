use color_eyre::eyre::eyre;
use nvim_oxi::api::opts::GetHighlightOpts;
use nvim_oxi::api::opts::GetHighlightOptsBuilder;
use nvim_oxi::api::opts::SetHighlightOpts;
use nvim_oxi::api::opts::SetHighlightOptsBuilder;
use nvim_oxi::api::types::GetHlInfos;
use nvim_oxi::api::types::HighlightInfos;

const BG: &str = "#001900";
const DIAGNOSTIC_LVLS: [&str; 5] = ["Error", "Warn", "Info", "Hint", "Ok"];
const STATUS_LINE_HL_BG: &str = "none";

pub fn set(_: ()) {
    let status_line_hl = set_opts().foreground("gray").background(STATUS_LINE_HL_BG).build();
    let bg_hl = set_opts().background(BG).build();

    let general_hls = [
        ("ColorColumn", set_opts().background("NvimDarkGrey3").build()),
        ("Cursor", set_opts().foreground("black").background("white").build()),
        ("CursorLine", set_opts().foreground("none").build()),
        ("MsgArea", status_line_hl.clone()),
        ("Normal", bg_hl.clone()),
        ("NormalFloat", bg_hl),
        ("StatusLine", status_line_hl),
    ];
    for (hl_name, hl_opts) in general_hls {
        set_hl(0, hl_name, &hl_opts);
    }

    let mut get_opts = get_opts();
    for lvl in DIAGNOSTIC_LVLS {
        let diagn_hl = format!("Diagnostic{lvl}");
        let Ok(hl_infos) = get_hl(0, &get_opts.name(diagn_hl.clone()).build()) else {
            continue;
        };
        let hl_opts = hl_opts_from_hl_infos(hl_infos).background(STATUS_LINE_HL_BG).build();
        set_hl(0, &format!("DiagnosticStatusLine{lvl}{diagn_hl}"), &hl_opts);

        let diagn_underline_hl = format!("DiagnosticUnderline{lvl}");
        let Ok(hl_infos) = get_hl(0, &get_opts.name(diagn_underline_hl.clone()).build()) else {
            continue;
        };
        let hl_opts = hl_opts_from_hl_infos(hl_infos).undercurl(true).build();
        set_hl(0, &diagn_underline_hl, &hl_opts);
    }
}

fn set_opts() -> SetHighlightOptsBuilder {
    SetHighlightOptsBuilder::default()
}

fn get_opts() -> GetHighlightOptsBuilder {
    GetHighlightOptsBuilder::default()
}

fn set_hl(ns_id: u32, hl_name: &str, hl_opts: &SetHighlightOpts) {
    if let Err(error) = nvim_oxi::api::set_hl(ns_id, hl_name, hl_opts) {
        crate::oxi_ext::notify_error(&format!("foo, error {error:#?}"));
    }
}

fn get_hl(ns_id: u32, hl_opts: &GetHighlightOpts) -> color_eyre::Result<HighlightInfos> {
    nvim_oxi::api::get_hl(ns_id, hl_opts)
        .inspect_err(|error| {
            crate::oxi_ext::notify_error(&format!("cannot get HighlightInfos by {hl_opts:#?}, error {error:#?}"))
        })
        .map_err(From::from)
        .and_then(|hl| match hl {
            GetHlInfos::Single(highlight_infos) => Ok(highlight_infos),
            GetHlInfos::Map(x) => Err(eyre!(
                "unexpected multiple HighlightInfos {:#?} for {hl_opts:#?}",
                x.collect::<Vec<_>>()
            )),
        })
}

fn hl_opts_from_hl_infos(hl_infos: HighlightInfos) -> SetHighlightOptsBuilder {
    let mut opts = set_opts();
    if let Some(value) = hl_infos.altfont {
        opts.altfont(value);
    }
    if let Some(value) = hl_infos.background.map(decimal_to_hex_color) {
        opts.background(&value);
    }
    if let Some(value) = hl_infos.bg_indexed {
        opts.bg_indexed(value);
    }
    if let Some(value) = hl_infos.blend {
        opts.blend(value as _);
    }
    if let Some(value) = hl_infos.bold {
        opts.bold(value);
    }
    if let Some(value) = hl_infos.fallback {
        opts.fallback(value);
    }
    if let Some(value) = hl_infos.fg_indexed {
        opts.fg_indexed(value);
    }
    if let Some(value) = hl_infos.force {
        opts.force(value);
    }
    if let Some(value) = hl_infos.foreground.map(decimal_to_hex_color) {
        opts.foreground(&value);
    }
    if let Some(value) = hl_infos.italic {
        opts.italic(value);
    }
    if let Some(value) = hl_infos.reverse {
        opts.reverse(value);
    }
    if let Some(value) = hl_infos.special.map(decimal_to_hex_color) {
        opts.special(&value);
    }
    if let Some(value) = hl_infos.standout {
        opts.standout(value);
    }
    if let Some(value) = hl_infos.strikethrough {
        opts.strikethrough(value);
    }
    if let Some(value) = hl_infos.undercurl {
        opts.undercurl(value);
    }
    if let Some(value) = hl_infos.underdash {
        opts.underdashed(value);
    }
    if let Some(value) = hl_infos.underdot {
        opts.underdotted(value);
    }
    if let Some(value) = hl_infos.underline {
        opts.underline(value);
    }
    if let Some(value) = hl_infos.underline {
        opts.underline(value);
    }
    opts
}

fn decimal_to_hex_color(decimal: u32) -> String {
    format!("#{:06X}", decimal)
}
