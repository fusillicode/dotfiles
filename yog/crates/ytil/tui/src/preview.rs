use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Arc;

use ratatui::text::Line;
use rootcause::report;
use skim::DisplayContext;
use skim::ItemPreview;
use skim::PreviewContext;
use skim::SkimItem;
use skim::options::SkimOptions;
use skim::prelude::SkimItemReader;
use skim::prelude::SkimItemReaderOption;

const PREVIEW_SEPARATOR_WIDTH: usize = 1;

#[derive(Debug)]
pub struct IndexedSkimItem {
    pub output: String,
    pub display_item: Arc<dyn SkimItem>,
    pub visible_text: String,
    pub preview_text: Option<String>,
    pub search_corpus: String,
}

impl SkimItem for IndexedSkimItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.visible_text)
    }

    fn display(&self, context: DisplayContext) -> Line<'_> {
        self.display_item.display(context)
    }

    fn preview(&self, context: PreviewContext) -> ItemPreview {
        self.preview_text.as_ref().map_or(ItemPreview::Global, |text| {
            ItemPreview::AnsiText(wrap_ansi_text(
                text,
                context.width.saturating_sub(PREVIEW_SEPARATOR_WIDTH),
            ))
        })
    }

    fn output(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.output)
    }
}

pub fn configure_options(options: &mut SkimOptions) {
    // Skim only creates a preview pane when a global preview is configured,
    // even when every item provides inline preview text.
    options.preview = Some(String::new());
    options.preview_window = "right:45%:wrap".into();
    options
        .bind
        .extend(["ctrl-d:preview-page-down".into(), "ctrl-u:preview-page-up".into()]);
}

pub fn build_ansi_display_items(display_texts: &[String]) -> rootcause::Result<Vec<Arc<dyn SkimItem>>> {
    let input = display_texts.join("\n");

    let reader_options = SkimItemReaderOption::default().ansi(true).build();
    let receiver = SkimItemReader::new(reader_options).of_bufread(Cursor::new(input));
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

fn wrap_ansi_text(text: &str, width: usize) -> String {
    if width == 0 {
        return text.to_owned();
    }

    let mut wrapped = String::with_capacity(text.len());
    let mut line_width: usize = 0;
    let mut index = 0;
    while index < text.len() {
        if text.as_bytes().get(index) == Some(&b'\x1b') {
            let escape_end = ansi_escape_end(text, index);
            if let Some(escape) = text.get(index..escape_end) {
                wrapped.push_str(escape);
            }
            index = escape_end;
            continue;
        }

        let Some(character) = text.get(index..).and_then(|suffix| suffix.chars().next()) else {
            break;
        };
        index = index.saturating_add(character.len_utf8());

        if character == '\n' {
            wrapped.push(character);
            line_width = 0;
            continue;
        }

        let character_width = Line::raw(character.to_string()).width();
        if line_width > 0 && line_width.saturating_add(character_width) > width {
            wrapped.push('\n');
            line_width = 0;
        }
        wrapped.push(character);
        line_width = line_width.saturating_add(character_width);
    }

    wrapped
}

fn ansi_escape_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut index = start.saturating_add(1);
    match bytes.get(index).copied() {
        Some(b'[') => {
            index = index.saturating_add(1);
            while let Some(byte) = bytes.get(index).copied() {
                index = index.saturating_add(1);
                if (b'@'..=b'~').contains(&byte) {
                    break;
                }
            }
        }
        Some(b']') => {
            index = index.saturating_add(1);
            while let Some(byte) = bytes.get(index).copied() {
                index = index.saturating_add(1);
                if byte == b'\x07' {
                    break;
                }
                if byte == b'\x1b' && bytes.get(index) == Some(&b'\\') {
                    index = index.saturating_add(1);
                    break;
                }
            }
        }
        Some(_) => {
            if let Some(character) = text.get(index..).and_then(|suffix| suffix.chars().next()) {
                index = index.saturating_add(character.len_utf8());
            }
        }
        None => {}
    }
    index
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use skim::DisplayContext;
    use skim::ItemPreview;
    use skim::PreviewContext;
    use skim::SkimItem;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_wrap_ansi_text_when_content_exceeds_width_inserts_scrollable_lines() {
        let text = "\u{1b}[31mabcdef\u{1b}[0m";

        assert_that!(wrap_ansi_text(text, 3), eq("\u{1b}[31mabc\ndef\u{1b}[0m"));
    }

    #[test]
    fn test_indexed_skim_item_when_display_and_preview_are_ansi_preserves_both_values() {
        let normalize = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
        let display = normalize("\u{1b}[31mvisible\tvalue\nnext\u{1b}[0m");
        let hidden_search = normalize("hidden\rvalue");
        let display_items_result = build_ansi_display_items(std::slice::from_ref(&display));
        assert_that!(display_items_result.as_ref().map(|_| ()), ok(eq(())));
        let mut display_items = display_items_result.expect("display item should build");
        let display_item = display_items.swap_remove(0);
        let match_text = format!("{} {hidden_search}", display_item.text());

        let item = IndexedSkimItem {
            output: "3".to_owned(),
            display_item,
            visible_text: "visible value next".to_owned(),
            preview_text: Some("\u{1b}[1mformatted preview\u{1b}[0m".to_owned()),
            search_corpus: match_text,
        };

        assert_that!(item.output(), eq("3"));
        assert_that!(item.text(), eq("visible value next"));
        assert_that!(
            matches!(
                item.preview(PreviewContext {
                    query: "",
                    cmd_query: "",
                    width: 0,
                    height: 0,
                    current_index: 0,
                    current_selection: "",
                    selected_indices: &[],
                    selections: &[],
                }),
                ItemPreview::AnsiText(text) if text == "\u{1b}[1mformatted preview\u{1b}[0m"
            ),
            eq(true)
        );
        assert_that!(
            item.display(DisplayContext::default())
                .spans
                .first()
                .map(|span| span.content.as_ref()),
            eq(Some("visible value next"))
        );
    }

    #[test]
    fn test_indexed_skim_item_when_preview_width_includes_one_separator_wraps_at_inner_width() {
        let item = IndexedSkimItem {
            output: "3".to_owned(),
            display_item: Arc::new("visible".to_owned()),
            visible_text: "visible".to_owned(),
            preview_text: Some("\u{1b}[31mabcdefghijklmnopq\u{1b}[0m".to_owned()),
            search_corpus: "visible".to_owned(),
        };

        assert_that!(
            matches!(
                item.preview(PreviewContext {
                    query: "",
                    cmd_query: "",
                    width: 10,
                    height: 2,
                    current_index: 0,
                    current_selection: "",
                    selected_indices: &[],
                    selections: &[],
                }),
                ItemPreview::AnsiText(text) if text == "\u{1b}[31mabcdefghi\njklmnopq\u{1b}[0m"
            ),
            eq(true)
        );
    }
}
