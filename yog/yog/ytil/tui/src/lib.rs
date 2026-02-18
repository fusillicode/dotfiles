//! Provide minimal TUI selection & prompt helpers built on [`skim`].
//!
//! Offer uniform, cancellable single / multi select prompts with fuzzy filtering and helpers
//! to derive a value from CLI args or fallback to an interactive selector.

use core::fmt::Debug;
use core::fmt::Display;
use std::io::Cursor;

use rootcause::report;
use skim::Skim;
use skim::SkimItem;
use skim::SkimOutput;
use skim::options::SkimOptions;
use skim::prelude::SkimItemReader;
use skim::prelude::SkimItemReaderOption;

pub mod git_branch;

/// Provides a minimal interactive multi-select prompt, returning [`Option::None`] if no options are
/// provided, the user cancels, or no items are selected.
///
/// # Errors
/// - [`skim`] fails to initialize or run.
pub fn minimal_multi_select<T: Display>(opts: Vec<T>) -> rootcause::Result<Option<Vec<T>>> {
    if opts.is_empty() {
        return Ok(None);
    }

    let (output, display_texts) = run_skim_prompt(&opts, select_options(true))?;
    if output.is_abort || output.selected_items.is_empty() {
        return Ok(None);
    }

    let mut selected_indices: Vec<usize> = output
        .selected_items
        .iter()
        .filter_map(|mi| find_selected_index(&display_texts, &mi.item))
        .collect();
    selected_indices.sort_unstable();

    let mut indexed_opts: Vec<Option<T>> = opts.into_iter().map(Some).collect();
    let selected: Vec<T> = selected_indices
        .into_iter()
        .filter_map(|i| indexed_opts.get_mut(i).and_then(Option::take))
        .collect();

    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
}

/// Minimal interactive single-select returning [`Option::None`] if `opts` is empty or the user cancels.
///
/// # Errors
/// - [`skim`] fails to initialize or run.
pub fn minimal_select<T: Display>(opts: Vec<T>) -> rootcause::Result<Option<T>> {
    if opts.is_empty() {
        return Ok(None);
    }

    let (output, display_texts) = run_skim_prompt(&opts, select_options(false))?;
    if output.is_abort || output.selected_items.is_empty() {
        return Ok(None);
    }

    let index = output
        .selected_items
        .first()
        .and_then(|mi| find_selected_index(&display_texts, &mi.item))
        .ok_or_else(|| report!("failed to recover selected item index"))?;

    opts.into_iter()
        .nth(index)
        .map(Some)
        .ok_or_else(|| report!("selected index out of bounds").attach(format!("index={index}")))
}

/// Displays a text input prompt with the given message, allowing cancellation via `Esc` / `Ctrl-C`.
///
/// # Errors
/// - [`skim`] fails to initialize or run.
pub fn text_prompt(message: &str) -> rootcause::Result<Option<String>> {
    let Some(output) = run_simple_prompt(simple_prompt_options(message, "3").build(), "")? else {
        return Ok(None);
    };
    let query = output.query.trim().to_owned();
    if query.is_empty() { Ok(None) } else { Ok(Some(query)) }
}

/// Displays a yes/no selection prompt.
///
/// # Errors
/// - [`skim`] fails to initialize or run.
pub fn yes_no_select(title: &str) -> rootcause::Result<Option<bool>> {
    let mut options = simple_prompt_options(title, "10%");
    options.no_sort = true;

    let Some(output) = run_simple_prompt(options.build(), "Yes\nNo")? else {
        return Ok(None);
    };
    if output.selected_items.is_empty() {
        return Ok(None);
    }

    let selected_text = output.selected_items.first().map(|mi| mi.item.output().into_owned());

    Ok(Some(selected_text.as_deref() == Some("Yes")))
}

/// Returns an item derived from CLI args or asks the user to select one.
///
/// Priority order:
/// 1. Tries to find the first CLI arg (by predicate) mapping to an existing item via `item_find_by_arg`.
/// 2. Falls back to interactive selection ([`minimal_select`]).
///
/// # Errors
/// - A CLI argument matches predicate but no corresponding item is found.
/// - The interactive selection fails (see [`minimal_select`]).
pub fn get_item_from_cli_args_or_select<'a, CAS, O, OBA, OF>(
    cli_args: &'a [String],
    mut cli_arg_selector: CAS,
    items: Vec<O>,
    item_find_by_arg: OBA,
) -> rootcause::Result<Option<O>>
where
    O: Clone + Debug + Display,
    CAS: FnMut(&(usize, &String)) -> bool,
    OBA: Fn(&'a str) -> OF,
    OF: FnMut(&O) -> bool + 'a,
{
    if let Some((_, cli_arg)) = cli_args.iter().enumerate().find(|x| cli_arg_selector(x)) {
        let mut item_find = item_find_by_arg(cli_arg);
        return Ok(Some(items.iter().find(|x| item_find(*x)).cloned().ok_or_else(
            || report!("missing item matching CLI arg").attach(format!("cli_arg={cli_arg} items={items:#?}")),
        )?));
    }
    minimal_select(items)
}

/// Runs [`skim`] with plain-text `input` lines and returns [`Option::None`] on abort.
fn run_simple_prompt(options: SkimOptions, input: &str) -> rootcause::Result<Option<SkimOutput>> {
    let items = SkimItemReader::default().of_bufread(Cursor::new(input.to_owned()));
    let output =
        Skim::run_with(options, Some(items)).map_err(|e| report!("skim failed to run").attach(e.to_string()))?;
    if output.is_abort {
        return Ok(None);
    }
    Ok(Some(output))
}

/// Feeds display-text items into [`skim`] via [`SkimItemReader`] and returns the selection output
/// alongside the original display texts for index recovery.
fn run_skim_prompt<T: Display>(opts: &[T], options: SkimOptions) -> rootcause::Result<(SkimOutput, Vec<String>)> {
    let display_texts: Vec<String> = opts.iter().map(ToString::to_string).collect();
    let input = display_texts.join("\n");
    let reader_opts = SkimItemReaderOption::from_options(&options);
    let items = SkimItemReader::new(reader_opts).of_bufread(Cursor::new(input));
    let output =
        Skim::run_with(options, Some(items)).map_err(|e| report!("skim failed to run").attach(e.to_string()))?;
    Ok((output, display_texts))
}

/// Recovers the original source-vector index by matching a selected item's output text against the
/// display texts collected before running skim.
fn find_selected_index(display_texts: &[String], item: &std::sync::Arc<dyn SkimItem>) -> Option<usize> {
    let output_text = item.output();
    display_texts.iter().position(|t| *t == *output_text)
}

/// Shared [`SkimOptions`] base: reverse layout, no info line, accept/abort keybindings.
fn base_skim_options() -> SkimOptions {
    let mut opts = SkimOptions::default();
    opts.reverse = true;
    opts.no_info = true;
    opts.bind = vec!["enter:accept".into(), "esc:abort".into(), "ctrl-c:abort".into()];
    opts
}

/// Lightweight prompt options with a visible prompt string and fixed height.
fn simple_prompt_options(prompt: &str, height: &str) -> SkimOptions {
    let mut opts = base_skim_options();
    opts.height = height.into();
    opts.prompt = format!("{prompt} ");
    opts
}

/// Configures [`SkimOptions`] for single or multi-select mode with ANSI support.
fn select_options(multi: bool) -> SkimOptions {
    let mut opts = base_skim_options();
    opts.multi = multi;
    opts.ansi = true;
    opts.height = "40%".into();
    if multi {
        opts.bind.extend(["ctrl-e:toggle".into(), "ctrl-a:toggle-all".into()]);
    }
    opts.build()
}
