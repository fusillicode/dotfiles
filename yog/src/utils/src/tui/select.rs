use color_eyre::eyre::eyre;
use inquire::InquireError;
use inquire::Select;
use skim::SkimItem;
use skim::SkimItemReceiver;
use skim::prelude::*;

use crate::tui::ClosablePrompt;
use crate::tui::ClosablePromptError;
use crate::tui::minimal_render_config;

pub fn get_items_via_sk<T: SkimItem + Clone>(items: Vec<T>) -> color_eyre::Result<Vec<T>> {
    let opts = base_skim_opts_builder(&mut SkimOptionsBuilder::default()).final_build()?;
    let source = build_skim_source_from_items(items)?;

    let Some(output) = Skim::run_with(&opts, Some(source)) else {
        return Ok(vec![]);
    };

    if output.is_abort {
        return Ok(vec![]);
    }

    let mut out = vec![];
    for item in output.selected_items {
        out.push(
            item.as_any()
                .downcast_ref::<T>()
                .cloned()
                .ok_or_else(|| eyre!("cannot downcast SkimItem to T"))?,
        );
    }
    Ok(out)
}

fn build_skim_source_from_items<T: SkimItem>(
    items: Vec<T>,
) -> color_eyre::Result<SkimItemReceiver> {
    let (tx, rx): (SkimItemSender, SkimItemReceiver) = unbounded();
    for item in items {
        tx.send(Arc::new(item))?;
    }
    Ok(rx)
}

fn base_skim_opts_builder(opts_builder: &mut SkimOptionsBuilder) -> &mut SkimOptionsBuilder {
    opts_builder
        .height(String::from("12"))
        .no_multi(true)
        .inline_info(true)
        .layout("reverse".into())
        .cycle(true)
}

pub fn minimal<'a, T: std::fmt::Display>(options: Vec<T>) -> Select<'a, T> {
    Select::new("", options)
        .with_render_config(minimal_render_config())
        .without_help_message()
}

/// Get the option matching a specific CLI argument or the one selected via an interactive
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
/// - `Ok(Some(option))` if an option is found by CLI argument or TUI selection
/// - `Ok(None)` if the user closes the TUI selection
/// - `Err` if no option if found by CLI argument or if TUI lookup fails
pub fn get_opt_from_cli_args_or_tui_select<'a, CAS, O, OBA, OF>(
    cli_args: &'a [String],
    mut cli_arg_selector: CAS,
    otps: Vec<O>,
    opt_find_by_arg: OBA,
) -> color_eyre::Result<Option<O>>
where
    O: Clone + std::fmt::Debug + std::fmt::Display,
    CAS: FnMut(&(usize, &String)) -> bool,
    OBA: Fn(&'a str) -> OF,
    OF: FnMut(&O) -> bool + 'a,
{
    if let Some((_, cli_arg)) = cli_args.iter().enumerate().find(|x| cli_arg_selector(x)) {
        let mut opt_find = opt_find_by_arg(cli_arg);
        return Ok(Some(
            otps.iter()
                .find(|x| opt_find(*x))
                .cloned()
                .ok_or_else(|| eyre!("no opt matches CLI arg {cli_arg} in opts {otps:#?}"))?,
        ));
    }
    match minimal::<O>(otps).closable_prompt() {
        Ok(opt) => Ok(Some(opt)),
        Err(ClosablePromptError::Closed) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl<'a, T: std::fmt::Display> ClosablePrompt<'a, T> for Select<'a, T> {
    fn closable_prompt(self) -> Result<T, ClosablePromptError> {
        self.prompt().map_or_else(
            |error| match error {
                InquireError::OperationCanceled | InquireError::OperationInterrupted => {
                    Err(ClosablePromptError::Closed)
                }
                InquireError::NotTTY
                | InquireError::InvalidConfiguration(_)
                | InquireError::IO(_)
                | InquireError::Custom(_) => Err(ClosablePromptError::Error(error)),
            },
            Result::Ok,
        )
    }
}
