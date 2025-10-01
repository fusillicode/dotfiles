//! Provide minimal TUI selection & prompt helpers built on `inquire`.
//!
//! Offer uniform, cancellable single / multi select prompts with stripped visual noise and helpers
//! to derive a value from CLI args or fallback to an interactive selector.
use color_eyre::eyre::eyre;
use inquire::InquireError;
use inquire::MultiSelect;
use inquire::Select;
use inquire::ui::RenderConfig;
use strum::EnumIter;
use strum::IntoEnumIterator;

/// Minimal interactive multi-select returning [`None`] if `opts` is empty or the user cancels.
///
/// Wraps [`inquire::MultiSelect`] with a slim rendering (see `minimal_render_config`) and no help message.
///
/// # Errors
/// In case:
/// - Rendering the prompt or terminal interaction inside [`inquire`] fails.
/// - Collecting the user selection fails for any reason reported by [`MultiSelect`].
pub fn minimal_multi_select<T: std::fmt::Display>(opts: Vec<T>) -> Result<Option<Vec<T>>, InquireError> {
    if opts.is_empty() {
        return Ok(None);
    }
    closable_prompt(
        MultiSelect::new("", opts)
            .with_render_config(minimal_render_config())
            .without_help_message()
            .prompt(),
    )
}

/// Minimal interactive single-select returning [`None`] if `opts` is empty or the user cancels.
///
/// Wraps [`inquire::Select`] with a slim rendering (see `minimal_render_config`) and no help message.
///
/// # Errors
/// In case:
/// - Rendering the prompt or terminal interaction inside [`inquire`] fails.
/// - Collecting the user selection fails for any reason reported by [`Select`].
pub fn minimal_select<T: std::fmt::Display>(opts: Vec<T>) -> Result<Option<T>, InquireError> {
    if opts.is_empty() {
        return Ok(None);
    }
    closable_prompt(
        Select::new("", opts)
            .with_render_config(minimal_render_config())
            .without_help_message()
            .prompt(),
    )
}

/// Displays a yes/no selection prompt with a minimal UI.
///
/// Returns `Ok(Some(_))` on selection, `Ok(None)` if canceled/interrupted.
///
/// # Errors
/// In case:
/// - Rendering the prompt or terminal interaction inside [`inquire`] fails.
pub fn yes_no_select(title: &str) -> Result<Option<bool>, InquireError> {
    closable_prompt(
        Select::new(title, YesNo::iter().collect())
            .with_render_config(minimal_render_config())
            .without_help_message()
            .prompt()
            .map(From::from),
    )
}

/// Represents a yes or no choice for user selection.
#[derive(Clone, Copy, Debug, EnumIter)]
enum YesNo {
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

impl core::fmt::Display for YesNo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let repr = match self {
            Self::Yes => "Yes",
            Self::No => "No",
        };
        write!(f, "{repr}")
    }
}

/// Returns an item derived from CLI args or asks the user to select one.
///
/// Priority order:
/// 1. Tries to find the first CLI arg (by predicate) mapping to an existing item via `item_find_by_arg`.
/// 2. Falls back to interactive selection (`minimal_select`).
///
/// Generic over a collection of displayable, cloneable items, so callers can pass any vector of choices.
///
/// # Type Parameters
/// * `CAS` - Closure filtering `(index, &String)` CLI arguments.
/// * `OBA` - Closure mapping an argument `&str` into a predicate over `&O`.
/// * `OF` - Predicate produced by `OBA` used to match an item.
///
/// # Errors
/// In case:
/// - A CLI argument matches predicate but no corresponding item is found.
/// - The interactive selection fails (see [`minimal_select`]).
pub fn get_item_from_cli_args_or_select<'a, CAS, O, OBA, OF>(
    cli_args: &'a [String],
    mut cli_arg_selector: CAS,
    items: Vec<O>,
    item_find_by_arg: OBA,
) -> color_eyre::Result<Option<O>>
where
    O: Clone + core::fmt::Debug + core::fmt::Display,
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
    Ok(minimal_select(items)?)
}

/// Converts an [`inquire`] prompt [`Result`] into an [`Option`]-wrapped [`Result`].
///
/// Treats [`InquireError::OperationCanceled`] / [`InquireError::OperationInterrupted`] as `Ok(None)`.
fn closable_prompt<T>(prompt_res: Result<T, InquireError>) -> Result<Option<T>, InquireError> {
    match prompt_res {
        Ok(res) => Ok(Some(res)),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Returns a minimalist [`RenderConfig`] with cleared prompt/answered prefixes.
fn minimal_render_config<'a>() -> RenderConfig<'a> {
    RenderConfig::default_colored()
        .with_prompt_prefix("".into())
        .with_canceled_prompt_indicator("".into())
        .with_answered_prompt_prefix("".into())
}
