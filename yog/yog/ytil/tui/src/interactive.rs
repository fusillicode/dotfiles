use std::borrow::Cow;
use std::fmt::Debug;
use std::fmt::Display;
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use ratatui::text::Line;
use rootcause::report;
use skim::DisplayContext;
use skim::MatchEngine;
use skim::MatchEngineFactory;
use skim::MatchRange;
use skim::MatchResult;
use skim::Skim;
use skim::SkimItem;
use skim::SkimItemReceiver;
use skim::SkimOutput;
use skim::matcher::Matcher as SkimMatcher;
use skim::options::SkimOptions;
use skim::prelude::SkimItemReader;
use skim::prelude::SkimItemReaderOption;
use skim::prelude::unbounded;

/// Provides a minimal interactive multi-select prompt.
///
/// Returns [`Option::None`] if no options are provided, the user cancels, or no items are selected.
/// Matching uses `search_text`, while rendering uses `display_text`.
///
/// # Errors
/// - [`skim`] fails to initialize or run.
pub fn minimal_multi_select<T, D, S>(
    opts: Vec<T>,
    mut display_text: D,
    mut search_text: S,
) -> rootcause::Result<Option<Vec<T>>>
where
    D: FnMut(&T) -> String,
    S: FnMut(&T) -> String,
{
    if opts.is_empty() {
        return Ok(None);
    }

    let normalize = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    let display_texts: Vec<String> = opts.iter().map(|opt| normalize(&display_text(opt))).collect();
    let display_items = build_ansi_display_items(&display_texts)?;
    let items: Vec<Arc<dyn SkimItem>> = opts
        .iter()
        .enumerate()
        .map(|(index, opt)| {
            let display_item = Arc::clone(display_items.get(index)?);
            let visible_match_text = display_item.text().into_owned();
            let hidden_search = normalize(&search_text(opt));
            let search_corpus = if hidden_search.is_empty() || hidden_search == visible_match_text {
                visible_match_text.clone()
            } else {
                format!("{visible_match_text} {hidden_search}")
            };

            Some(Arc::new(IndexedSkimItem {
                output: index.to_string(),
                display_item,
                visible_text: visible_match_text,
                search_corpus,
            }) as Arc<dyn SkimItem>)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| report!("missing ANSI display item while building skim rows"))?;

    let (tx_items, rx_items) = unbounded();
    tx_items
        .send(items)
        .map_err(|e| report!("failed to queue skim items").attach(e.to_string()))?;
    drop(tx_items);

    let options = select_options(true);
    let output = run_skim_with_matcher(options, rx_items)?;

    if output.is_abort || output.selected_items.is_empty() {
        return Ok(None);
    }

    let mut selected_indices: Vec<usize> = output
        .selected_items
        .iter()
        .filter_map(|mi| mi.item.output().parse().ok())
        .collect();
    selected_indices.sort_unstable();
    selected_indices.dedup();

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
        .and_then(|mi| {
            let output_text = mi.item.output();
            display_texts.iter().position(|t| *t == *output_text)
        })
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
    let options = simple_prompt_options(title, "10%");

    let Some(output) = run_simple_prompt(options.build(), "Yes\nNo")? else {
        return Ok(None);
    };
    if output.selected_items.is_empty() {
        return Ok(None);
    }

    let selected_text = output.selected_items.first().map(|mi| mi.item.output().into_owned());

    Ok(Some(selected_text.as_deref() == Some("Yes")))
}

/// Require exactly one selected item.
///
/// # Errors
/// - More than one item is selected.
/// - No items are selected.
pub fn require_single<'a, T>(selected: &'a [T], item_name_plural: &str) -> rootcause::Result<&'a T> {
    let [item] = selected else {
        return Err(report!("expected exactly one selection")
            .attach(format!("item_name_plural={item_name_plural}"))
            .attach(format!("selected_count={}", selected.len())));
    };
    Ok(item)
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

#[derive(Debug)]
struct IndexedSkimItem {
    output: String,
    display_item: Arc<dyn SkimItem>,
    visible_text: String,
    search_corpus: String,
}

impl SkimItem for IndexedSkimItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.visible_text)
    }

    fn display(&self, context: DisplayContext) -> Line<'_> {
        self.display_item.display(context)
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.output)
    }
}

struct SearchCorpusEngineFactory {
    inner: Rc<dyn MatchEngineFactory>,
}

impl SearchCorpusEngineFactory {
    fn new(inner: Rc<dyn MatchEngineFactory>) -> Self {
        Self { inner }
    }
}

impl MatchEngineFactory for SearchCorpusEngineFactory {
    fn create_engine_with_case(&self, query: &str, case: skim::CaseMatching) -> Box<dyn MatchEngine> {
        Box::new(SearchCorpusEngine {
            inner: self.inner.create_engine_with_case(query, case),
        })
    }
}

struct SearchCorpusEngine {
    inner: Box<dyn MatchEngine>,
}

impl Display for SearchCorpusEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl MatchEngine for SearchCorpusEngine {
    fn match_item(&self, item: &dyn SkimItem) -> Option<MatchResult> {
        let Some(item) = item.as_any().downcast_ref::<IndexedSkimItem>() else {
            return self.inner.match_item(item);
        };

        let mut result = self.inner.match_item(&item.search_corpus)?;
        result.matched_range = clip_match_range(result.matched_range, item);
        Some(result)
    }
}

fn clip_match_range(match_range: MatchRange, item: &IndexedSkimItem) -> MatchRange {
    let visible_char_len = item.visible_text.chars().count();
    let visible_byte_len = item.visible_text.len();

    match match_range {
        MatchRange::Chars(indices) => {
            MatchRange::Chars(indices.into_iter().filter(|index| *index < visible_char_len).collect())
        }
        MatchRange::CharRange(start, end) => {
            if start >= visible_char_len {
                MatchRange::Chars(Vec::new())
            } else {
                MatchRange::CharRange(start, end.min(visible_char_len))
            }
        }
        MatchRange::ByteRange(start, end) => {
            if start >= visible_byte_len {
                MatchRange::Chars(Vec::new())
            } else {
                MatchRange::ByteRange(start, end.min(visible_byte_len))
            }
        }
    }
}

fn run_skim_with_matcher(options: SkimOptions, source: SkimItemReceiver) -> rootcause::Result<SkimOutput> {
    let (engine_factory, rank_builder) = SkimMatcher::create_engine_factory_with_builder(&options);
    let matcher = SkimMatcher::builder(Rc::new(SearchCorpusEngineFactory::new(engine_factory)))
        .case(options.case)
        .rank_builder(rank_builder)
        .build();

    let mut skim = skim::Skim::init(options, Some(source))
        .map_err(|e| report!("skim failed to initialize").attach(e.to_string()))?;
    skim.app_mut().matcher = matcher;
    skim.start();

    if skim.should_enter() {
        skim.init_tui()
            .map_err(|e| report!("skim failed to initialize TUI").attach(e.to_string()))?;

        let task = async {
            skim.enter().await.map_err(|e| e.to_string())?;
            skim.run().await.map_err(|e| e.to_string())?;
            Ok::<(), String>(())
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| handle.block_on(task))
                .map_err(|e| report!("skim failed to run").attach(e))?;
        } else {
            tokio::runtime::Runtime::new()
                .map_err(|e| report!("failed to create tokio runtime").attach(e.to_string()))?
                .block_on(task)
                .map_err(|e| report!("skim failed to run").attach(e))?;
        }
    }

    Ok(skim.output())
}

fn build_ansi_display_items(display_texts: &[String]) -> rootcause::Result<Vec<Arc<dyn SkimItem>>> {
    let input = display_texts.join("\n");

    let reader_opts = SkimItemReaderOption::default().ansi(true).build();
    let receiver = SkimItemReader::new(reader_opts).of_bufread(Cursor::new(input));
    let mut items = Vec::with_capacity(display_texts.len());
    while let Ok(batch) = receiver.recv() {
        items.extend(batch);
    }

    if items.len() != display_texts.len() {
        return Err(report!("failed to build ANSI display items")
            .attach(format!("expected={}", display_texts.len()))
            .attach(format!("actual={}", items.len())));
    }
    Ok(items)
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

/// Shared [`SkimOptions`] base: reverse layout, no info line, accept/abort keybindings,
/// input-order preserved during filtering.
fn base_skim_options() -> SkimOptions {
    let mut opts = SkimOptions::default();
    opts.reverse = true;
    opts.no_info = true;
    opts.exact = true;
    opts.no_sort = true;
    opts.cycle = true;
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pretty_assertions::assert_eq;
    use skim::DisplayContext;
    use skim::MatchRange;
    use skim::SkimItem;

    #[test]
    fn test_require_single_returns_only_item() {
        let selected = vec![1];
        assert2::assert!(let Ok(item) = super::require_single(&selected, "items"));
        assert_eq!(*item, 1);
    }

    #[test]
    fn test_require_single_errors_for_multiple_items() {
        let selected = vec![1, 2];
        assert2::assert!(let Err(err) = super::require_single(&selected, "items"));
        assert!(err.to_string().contains("expected exactly one selection"));
    }

    #[test]
    fn test_minimal_multi_select_line_serialization_sanitizes_fields() {
        let normalize = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
        let display = normalize("\u{1b}[31mvisible\tvalue\nnext\u{1b}[0m");
        let hidden_search = normalize("hidden\rvalue");
        assert2::assert!(let Ok(mut display_items) = super::build_ansi_display_items(std::slice::from_ref(&display)));
        let display_item = display_items.swap_remove(0);
        let match_text = format!("{} {hidden_search}", display_item.text());

        let item = super::IndexedSkimItem {
            output: "3".to_owned(),
            display_item,
            visible_text: "visible value next".to_owned(),
            search_corpus: match_text,
        };

        assert_eq!(item.output(), "3");
        assert_eq!(item.text(), "visible value next");
        assert_eq!(
            item.display(DisplayContext::default())
                .spans
                .first()
                .map(|span| span.content.as_ref()),
            Some("visible value next")
        );
    }

    #[test]
    fn test_clip_match_range_char_indices_hides_hidden_only_match() {
        let item = super::IndexedSkimItem {
            output: "3".to_owned(),
            display_item: Arc::new("visible value next".to_owned()),
            visible_text: "visible value next".to_owned(),
            search_corpus: "visible value next hidden value".to_owned(),
        };

        assert!(matches!(
            super::clip_match_range(MatchRange::Chars(vec![20, 21]), &item),
            MatchRange::Chars(indices) if indices.is_empty()
        ));
    }
}
