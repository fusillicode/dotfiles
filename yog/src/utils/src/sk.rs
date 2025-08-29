use color_eyre::eyre::eyre;
pub use skim::ItemPreview as SkimItemPreview;
pub use skim::PreviewContext as SkimPreviewContext;
pub use skim::SkimItem;
use skim::prelude::*;

/// Prompts the user to select either [`YesNo::Yes`] or [`YesNo::No`] using the skim fuzzy finder.
/// Returns [`Option::Some`][YesNo] if an option is selected, or [`Option::None`] if the selection is canceled.
pub fn select_yes_or_no(prompt: String) -> color_eyre::Result<Option<YesNo>> {
    let sk_opts = base_sk_opts(&mut Default::default())
        .prompt(prompt)
        .preview(None)
        .no_clear_start(true)
        .final_build()?;
    get_item(vec![YesNo::Yes, YesNo::No], Some(sk_opts))
}

/// Represents a yes or no choice for user selection.
#[derive(Clone, Copy, Debug)]
pub enum YesNo {
    Yes,
    No,
}

impl From<YesNo> for bool {
    fn from(value: YesNo) -> Self {
        match value {
            YesNo::Yes => true,
            YesNo::No => false,
        }
    }
}

impl SkimItem for YesNo {
    fn text(&self) -> std::borrow::Cow<'_, str> {
        Cow::from(match self {
            YesNo::Yes => "Yes",
            YesNo::No => "No",
        })
    }
}

/// Get the selected item among the supplied ones with sk configured with either:
/// - the supplied [SkimOptionsBuilder] or
/// - the base one [base_sk_opts]
pub fn get_item<T: SkimItem + Clone + core::fmt::Debug>(
    items: Vec<T>,
    sk_opts: Option<SkimOptions>,
) -> color_eyre::Result<Option<T>> {
    match &get_items(items, sk_opts)?.as_slice() {
        &[item] => Ok(Some(item.clone())),
        [] => Ok(None),
        multiple_items => Err(eyre!("unexpected multiple selected items {multiple_items:#?}")),
    }
}

/// Get the item matching a specific CLI argument or the one selected via an interactive
/// sk selection.
///
/// # Behavior
/// 1. CLI arguments flow:
///    - uses `cli_arg_selector` to find a specific CLI argument
///    - returns first matching option or error if none found
///
/// 2. Interactive sk flow:
///    - falls back to sk selection if no CLI argument matches
///    - returns user selection or None if dialog closed
///
/// # Returns
/// - `Ok(Some(option))` if an item is found by CLI argument or sk selection
/// - `Ok(None)` if the user closes the sk selection
/// - `Err` if no item if found by CLI argument or if sk lookup fails
pub fn get_item_from_cli_args_or_sk_select<'a, CAS, O, OBA, OF>(
    cli_args: &'a [String],
    mut cli_arg_selector: CAS,
    items: Vec<O>,
    item_find_by_arg: OBA,
    sk_opts: Option<SkimOptions>,
) -> color_eyre::Result<Option<O>>
where
    O: SkimItem + Clone + core::fmt::Debug + core::fmt::Display,
    CAS: FnMut(&(usize, &String)) -> bool,
    OBA: Fn(&'a str) -> OF,
    OF: FnMut(&O) -> bool + 'a,
{
    if let Some((_, cli_arg)) = cli_args.iter().enumerate().find(|x| cli_arg_selector(x)) {
        let mut item_find = item_find_by_arg(cli_arg);
        return Ok(Some(items.iter().find(|x| item_find(*x)).cloned().ok_or_else(
            || eyre!("missing item matches CLI arg {cli_arg} in opts {items:#?}"),
        )?));
    }
    get_item(items, sk_opts)
}

/// Runs the skim fuzzy finder with the provided items and returns the selected items.
/// Returns an empty vector if the selection is aborted or cancelled.
fn get_items<T: SkimItem + Clone + core::fmt::Debug>(
    items: Vec<T>,
    sk_opts: Option<SkimOptions>,
) -> color_eyre::Result<Vec<T>> {
    let sk_opts = match sk_opts {
        Some(opts) => opts,
        None => {
            let mut sk_opts = SkimOptionsBuilder::default();
            base_sk_opts(&mut sk_opts);
            sk_opts.final_build()?
        }
    };
    let sk_source = build_sk_source_from_items(items)?;

    let Some(sk_output) = Skim::run_with(&sk_opts, Some(sk_source)) else {
        return Ok(vec![]);
    };

    if sk_output.is_abort {
        return Ok(vec![]);
    }

    let mut out = vec![];
    for item in sk_output.selected_items {
        out.push(
            (*item)
                .as_any()
                .downcast_ref::<T>()
                .cloned()
                .ok_or_else(|| eyre!("cannot downcast SkimItem to type {}", std::any::type_name::<T>()))?,
        );
    }
    Ok(out)
}

/// Creates a skim item receiver from a vector of items for use with the fuzzy finder.
/// The receiver can be used as a source for the skim fuzzy finder.
fn build_sk_source_from_items<T: SkimItem>(items: Vec<T>) -> color_eyre::Result<SkimItemReceiver> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    for item in items {
        tx.send(Arc::new(item))?;
    }
    Ok(rx)
}

/// Configures the base skim options with common settings for a consistent user experience.
fn base_sk_opts(opts_builder: &mut SkimOptionsBuilder) -> &mut SkimOptionsBuilder {
    opts_builder
        .height(String::from("21"))
        .no_multi(true)
        .inline_info(true)
        .layout("reverse".into())
        .preview(Some("".into()))
        .preview_window("down:50%".into())
        .color(Some("16,prompt:#ffffff".into()))
        .cycle(true)
}
