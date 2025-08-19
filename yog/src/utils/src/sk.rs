use color_eyre::eyre::eyre;
use skim::prelude::*;

pub use skim::SkimItem;

/// Get the selected item among the supplied ones via a commonly configured skim.
pub fn get_item<T: SkimItem + Clone + std::fmt::Debug>(
    items: Vec<T>,
) -> color_eyre::Result<Option<T>> {
    match &get_items(items)?.as_slice() {
        &[item] => Ok(Some(item.clone())),
        [] => Ok(None),
        multiple_items => Err(eyre!(
            "unexpected multiple selected items {multiple_items:#?}"
        )),
    }
}

/// Get the item matching a specific CLI argument or the one selected via an interactive
/// TUI selection.
///
/// # Behavior
/// 1. CLI arguments flow:
///    - uses `cli_arg_selector` to find a specific CLI argument
///    - returns first matching option or error if none found
///
/// 2. Interactive TUI flow:
///    - falls back to TUI selection if no CLI argument matches
///    - returns user selection or None if dialog closed
///
/// # Returns
/// - `Ok(Some(option))` if an item is found by CLI argument or TUI selection
/// - `Ok(None)` if the user closes the TUI selection
/// - `Err` if no item if found by CLI argument or if TUI lookup fails
pub fn get_item_from_cli_args_or_tui_select<'a, CAS, O, OBA, OF>(
    cli_args: &'a [String],
    mut cli_arg_selector: CAS,
    items: Vec<O>,
    item_find_by_arg: OBA,
) -> color_eyre::Result<Option<O>>
where
    O: SkimItem + Clone + std::fmt::Debug + std::fmt::Display,
    CAS: FnMut(&(usize, &String)) -> bool,
    OBA: Fn(&'a str) -> OF,
    OF: FnMut(&O) -> bool + 'a,
{
    if let Some((_, cli_arg)) = cli_args.iter().enumerate().find(|x| cli_arg_selector(x)) {
        let mut item_find = item_find_by_arg(cli_arg);
        return Ok(Some(
            items
                .iter()
                .find(|x| item_find(*x))
                .cloned()
                .ok_or_else(|| eyre!("no item matches CLI arg {cli_arg} in opts {items:#?}"))?,
        ));
    }
    get_item(items)
}

fn get_items<T: SkimItem + Clone + std::fmt::Debug>(items: Vec<T>) -> color_eyre::Result<Vec<T>> {
    let sk_opts = common_sk_opts(&mut SkimOptionsBuilder::default()).final_build()?;
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
                .ok_or_else(|| {
                    eyre!(
                        "cannot downcast SkimItem to type {}",
                        std::any::type_name::<T>()
                    )
                })?,
        );
    }
    Ok(out)
}

fn build_sk_source_from_items<T: SkimItem>(items: Vec<T>) -> color_eyre::Result<SkimItemReceiver> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    for item in items {
        tx.send(Arc::new(item))?;
    }
    Ok(rx)
}

fn common_sk_opts(opts_builder: &mut SkimOptionsBuilder) -> &mut SkimOptionsBuilder {
    opts_builder
        .height(String::from("12"))
        .no_multi(true)
        .inline_info(true)
        .layout("reverse".into())
        .cycle(true)
}
