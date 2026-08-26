use muxr_core::RenderHyperlink;
use muxr_core::RenderRowSpan;
use muxr_core::RowWrap;
use rootcause::report;
use url::Url;

const HTTP_SCHEME: &str = "http://";
const HTTPS_SCHEME: &str = "https://";

#[derive(Clone, Debug, Eq, PartialEq)]
struct UrlCandidate {
    end: UrlCandidateEnd,
    positions: Vec<CellPosition>,
    text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlCandidateEnd {
    Delimiter,
    VisibleEdge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CellPosition {
    cell: usize,
    row: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneUrlLink {
    hyperlink: RenderHyperlink,
    position: CellPosition,
}

impl PaneUrlLink {
    pub const fn cell(&self) -> usize {
        self.position.cell
    }

    pub fn into_hyperlink(self) -> RenderHyperlink {
        self.hyperlink
    }

    pub const fn row(&self) -> usize {
        self.position.row
    }

    fn offset_row(mut self, offset: usize) -> rootcause::Result<Self> {
        self.position.row = self
            .position
            .row
            .checked_add(offset)
            .ok_or_else(|| report!("muxr pane URL link row overflowed"))?;
        Ok(self)
    }
}

pub fn detect_visible_url_links(rows: &[RenderRowSpan], row_wraps: &[RowWrap]) -> rootcause::Result<Vec<PaneUrlLink>> {
    let mut links = Vec::new();
    let mut linked_cells = rows
        .iter()
        .map(|row| vec![LinkCellState::Unlinked; row.cells().len()])
        .collect::<Vec<_>>();
    for (row, span) in rows.iter().enumerate() {
        for cell in 0..span.cells().len() {
            let position = CellPosition { cell, row };
            if LinkCellState::at(&linked_cells, position) == LinkCellState::Linked {
                continue;
            }
            let Some(candidate) = self::url_candidate_at(rows, row_wraps, position) else {
                continue;
            };
            let Some(uri) = self::valid_url_prefix(&candidate) else {
                continue;
            };
            if candidate
                .positions
                .iter()
                .take(uri.len())
                .any(|position| self::cell_has_explicit_hyperlink(rows, *position))
            {
                continue;
            }
            let hyperlink = RenderHyperlink::new(uri.clone())?;
            for position in candidate.positions.into_iter().take(uri.len()) {
                self::mark_cell_linked(&mut linked_cells, position)?;
                links.push(PaneUrlLink {
                    hyperlink: hyperlink.clone(),
                    position,
                });
            }
        }
    }

    Ok(links)
}

pub fn detect_visible_url_links_for_rows(
    rows: &[RenderRowSpan],
    row_wraps: &[RowWrap],
    selected_rows: &[u16],
) -> rootcause::Result<Vec<PaneUrlLink>> {
    let selected_rows = self::expand_visible_url_rows(rows, row_wraps, selected_rows);
    let mut links = Vec::new();
    let mut next = 0;
    while let Some(start) = selected_rows.get(next).copied() {
        let start = usize::from(start);
        let mut end_index = next.saturating_add(1);
        while selected_rows.get(end_index).is_some_and(|row| {
            let expected = start.saturating_add(end_index.saturating_sub(next));
            usize::from(*row) == expected
        }) {
            end_index = end_index.saturating_add(1);
        }
        let end = start.saturating_add(end_index.saturating_sub(next));
        let range_rows = rows
            .get(start..end)
            .ok_or_else(|| report!("muxr pane URL dependency rows are outside the visible snapshot"))?;
        let range_wraps = row_wraps
            .get(start..end)
            .ok_or_else(|| report!("muxr pane URL dependency wraps are outside the visible snapshot"))?;
        for link in self::detect_visible_url_links(range_rows, range_wraps)? {
            links.push(link.offset_row(start)?);
        }
        next = end_index;
    }
    Ok(links)
}

pub fn expand_visible_url_rows(rows: &[RenderRowSpan], row_wraps: &[RowWrap], changed_rows: &[u16]) -> Vec<u16> {
    if rows.is_empty() {
        return Vec::new();
    }

    let mut changed = vec![false; rows.len()];
    for changed_row in changed_rows {
        let changed_row = usize::from(*changed_row);
        if let Some(changed) = changed.get_mut(changed_row) {
            *changed = true;
        }
    }

    let mut selected = Vec::new();
    let mut component_start = 0_usize;
    let mut changed_rows = changed.into_iter().enumerate();
    let Some((_first_row, mut component_changed)) = changed_rows.next() else {
        return selected;
    };
    for (row, row_changed) in changed_rows {
        if self::rows_share_url_candidate(rows, row_wraps, row.saturating_sub(1)) {
            component_changed |= row_changed;
            continue;
        }
        self::append_changed_component(&mut selected, component_start..row, component_changed);
        component_start = row;
        component_changed = row_changed;
    }
    self::append_changed_component(&mut selected, component_start..rows.len(), component_changed);
    selected
}

fn append_changed_component(selected: &mut Vec<u16>, rows: std::ops::Range<usize>, changed: bool) {
    if changed {
        selected.extend(rows.filter_map(|row| u16::try_from(row).ok()));
    }
}

fn cell_has_explicit_hyperlink(rows: &[RenderRowSpan], position: CellPosition) -> bool {
    rows.get(position.row)
        .and_then(|row| row.cells().get(position.cell))
        .is_some_and(|cell| cell.hyperlink().is_some())
}

fn rows_share_url_candidate(rows: &[RenderRowSpan], row_wraps: &[RowWrap], upper_row: usize) -> bool {
    if row_wraps.get(upper_row) != Some(&RowWrap::EndsWithSoftWrap) {
        return false;
    }
    let Some(upper) = rows.get(upper_row) else {
        return false;
    };
    let Some(lower_row) = upper_row.checked_add(1) else {
        return false;
    };
    let Some(lower) = rows.get(lower_row) else {
        return false;
    };
    let Some(upper_cell) = upper.cells().len().checked_sub(1) else {
        return false;
    };
    UrlChar::from_char(
        self::cell_ascii_char(
            rows,
            CellPosition {
                row: upper_row,
                cell: upper_cell,
            },
        )
        .unwrap_or(' '),
    ) == UrlChar::Url
        && UrlChar::from_char(
            self::cell_ascii_char(
                rows,
                CellPosition {
                    row: lower_row,
                    cell: 0,
                },
            )
            .unwrap_or(' '),
        ) == UrlChar::Url
        && !lower.cells().is_empty()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkCellState {
    Linked,
    Unlinked,
}

impl LinkCellState {
    fn at(linked_cells: &[Vec<Self>], position: CellPosition) -> Self {
        linked_cells
            .get(position.row)
            .and_then(|row| row.get(position.cell))
            .copied()
            .unwrap_or(Self::Unlinked)
    }
}

fn mark_cell_linked(linked_cells: &mut [Vec<LinkCellState>], position: CellPosition) -> rootcause::Result<()> {
    let Some(cell) = linked_cells
        .get_mut(position.row)
        .and_then(|row| row.get_mut(position.cell))
    else {
        return Err(report!("muxr pane url link position is outside visible rows"));
    };
    *cell = LinkCellState::Linked;
    Ok(())
}

fn url_candidate_at(rows: &[RenderRowSpan], row_wraps: &[RowWrap], position: CellPosition) -> Option<UrlCandidate> {
    let scheme_len = self::scheme_len_at(rows, row_wraps, position)?;
    let mut positions = Vec::new();
    let mut text = String::new();
    let mut current = Some(position);
    let end = loop {
        let Some(position) = current else {
            break UrlCandidateEnd::VisibleEdge;
        };
        let Some(ch) = self::cell_ascii_char(rows, position) else {
            break UrlCandidateEnd::Delimiter;
        };
        if UrlChar::from_char(ch) == UrlChar::Delimiter {
            break UrlCandidateEnd::Delimiter;
        }
        positions.push(position);
        text.push(ch);
        current = self::next_position(rows, row_wraps, position);
    };

    if text.len() <= scheme_len {
        return None;
    }

    Some(UrlCandidate { end, positions, text })
}

fn scheme_len_at(rows: &[RenderRowSpan], row_wraps: &[RowWrap], position: CellPosition) -> Option<usize> {
    [HTTPS_SCHEME, HTTP_SCHEME]
        .into_iter()
        .find(|scheme| UrlSchemeMatch::at(rows, row_wraps, position, scheme) == UrlSchemeMatch::Matched)
        .map(str::len)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlSchemeMatch {
    Matched,
    Missing,
}

impl UrlSchemeMatch {
    fn at(rows: &[RenderRowSpan], row_wraps: &[RowWrap], position: CellPosition, scheme: &str) -> Self {
        let mut current = Some(position);
        for expected in scheme.chars() {
            let Some(position) = current else {
                return Self::Missing;
            };
            if self::cell_ascii_char(rows, position) != Some(expected) {
                return Self::Missing;
            }
            current = self::next_position(rows, row_wraps, position);
        }
        Self::Matched
    }
}

fn valid_url_prefix(candidate: &UrlCandidate) -> Option<String> {
    let trimmed = self::trim_url_candidate(&candidate.text);
    let url = Url::parse(trimmed).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if VisibleEdgeFragment::from_candidate(candidate, trimmed, &url) == VisibleEdgeFragment::Ambiguous {
        return None;
    }
    Some(trimmed.to_owned())
}

fn trim_url_candidate(text: &str) -> &str {
    let mut trimmed = text.trim_end_matches(|ch| TrailingPunctuation::from_char(ch) == TrailingPunctuation::Yes);
    let mut unmatched_closing_parentheses = trimmed
        .matches(')')
        .count()
        .saturating_sub(trimmed.matches('(').count());
    // Keep balanced parentheses in URL paths, but exclude unmatched closing wrappers from the hyperlink target.
    while unmatched_closing_parentheses > 0 && trimmed.ends_with(')') {
        let Some(without_wrapper) = trimmed.strip_suffix(')') else {
            break;
        };
        trimmed = without_wrapper.trim_end_matches(|ch| TrailingPunctuation::from_char(ch) == TrailingPunctuation::Yes);
        unmatched_closing_parentheses = unmatched_closing_parentheses.saturating_sub(1);
    }
    trimmed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisibleEdgeFragment {
    Ambiguous,
    Complete,
}

impl VisibleEdgeFragment {
    fn from_candidate(candidate: &UrlCandidate, text: &str, url: &Url) -> Self {
        if candidate.end != UrlCandidateEnd::VisibleEdge {
            return Self::Complete;
        }
        // `url` accepts bare single-label hosts such as `https://exam`; at the visible edge those are often just the
        // first row of a wrapped URL, so reject only that ambiguous bare-authority case.
        if HostBoundarySignal::from_url(url) == HostBoundarySignal::Present
            || UrlAuthorityShape::from_url_text(text, url) != UrlAuthorityShape::Bare
        {
            return Self::Complete;
        }
        Self::Ambiguous
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlAuthorityShape {
    Bare,
    NotBare,
}

impl UrlAuthorityShape {
    fn from_url_text(text: &str, url: &Url) -> Self {
        let Some(authority_start) = url.scheme().len().checked_add("://".len()) else {
            return Self::NotBare;
        };
        let Some(after_scheme) = text.get(authority_start..) else {
            return Self::NotBare;
        };
        let Some(host) = url.host_str() else {
            return Self::Bare;
        };
        if after_scheme == host {
            Self::Bare
        } else {
            Self::NotBare
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostBoundarySignal {
    Missing,
    Present,
}

impl HostBoundarySignal {
    fn from_url(url: &Url) -> Self {
        let Some(host) = url.host_str() else {
            return Self::Missing;
        };
        if host == "localhost" || host.contains('.') || url.port().is_some() {
            Self::Present
        } else {
            Self::Missing
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrailingPunctuation {
    No,
    Yes,
}

impl TrailingPunctuation {
    const fn from_char(ch: char) -> Self {
        if matches!(ch, '.' | ',' | ';' | '!' | '?') {
            Self::Yes
        } else {
            Self::No
        }
    }
}

fn next_position(rows: &[RenderRowSpan], row_wraps: &[RowWrap], position: CellPosition) -> Option<CellPosition> {
    let next_cell = position.cell.checked_add(1)?;
    if next_cell < rows.get(position.row)?.cells().len() {
        return Some(CellPosition {
            row: position.row,
            cell: next_cell,
        });
    }

    if row_wraps.get(position.row) != Some(&RowWrap::EndsWithSoftWrap) {
        return None;
    }

    let next_row = position.row.checked_add(1)?;
    rows.get(next_row)
        .and_then(|row| (!row.cells().is_empty()).then_some(CellPosition { row: next_row, cell: 0 }))
}

fn cell_ascii_char(rows: &[RenderRowSpan], position: CellPosition) -> Option<char> {
    let text = rows.get(position.row)?.cells().get(position.cell)?.text();
    let mut chars = text.chars();
    let ch = chars.next()?;
    (chars.next().is_none() && ch.is_ascii()).then_some(ch)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlChar {
    Delimiter,
    Url,
}

impl UrlChar {
    const fn from_char(ch: char) -> Self {
        if ch.is_ascii_alphanumeric()
            || matches!(
                ch,
                '-' | '.'
                    | '_'
                    | '~'
                    | ':'
                    | '/'
                    | '?'
                    | '#'
                    | '@'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
            )
        {
            Self::Url
        } else {
            Self::Delimiter
        }
    }
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderCell;
    use muxr_core::RenderStyle;
    use rootcause::option_ext::OptionExt;
    use rootcause::prelude::ResultExt;
    use rstest::rstest;
    use test_that::prelude::*;

    use super::*;

    #[rstest]
    #[case::http("http://example.com")]
    #[case::https("https://example.com")]
    #[case::localhost("http://localhost:3000")]
    #[case::explicit_port_at_edge("http://grafana:3000")]
    #[case::balanced_parentheses("https://example.com/a_(b)")]
    fn test_link_visible_urls_when_plain_url_is_visible_links_url_cells(#[case] uri: &str) -> rootcause::Result<()> {
        let rows = self::rows(&[&format!("go {uri}")])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        self::assert_linked_text(&rows, &links, uri, uri);
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_single_label_host_has_delimiter_links_url() -> rootcause::Result<()> {
        let rows = self::rows(&["go http://grafana now"])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        self::assert_linked_text(&rows, &links, "http://grafana", "http://grafana");
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_single_label_host_has_explicit_path_at_edge_links_url() -> rootcause::Result<()> {
        let rows = self::rows(&["http://grafana/"])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        self::assert_linked_text(&rows, &links, "http://grafana/", "http://grafana/");
        Ok(())
    }

    #[rstest]
    #[case::period("https://example.com.", "https://example.com")]
    #[case::comma("https://example.com,", "https://example.com")]
    #[case::semicolon("https://example.com;", "https://example.com")]
    #[case::bang("https://example.com!", "https://example.com")]
    #[case::question("https://example.com?", "https://example.com")]
    #[case::closing_parenthesis("(https://example.com)", "https://example.com")]
    #[case::multiple_closing_parentheses("(https://example.com)))", "https://example.com")]
    fn test_link_visible_urls_when_url_has_trailing_punctuation_trims_link_target(
        #[case] text: &str,
        #[case] uri: &str,
    ) -> rootcause::Result<()> {
        let rows = self::rows(&[text])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        self::assert_linked_text(&rows, &links, uri, uri);
        let uri_start = text.find(uri).context("muxr test URI is missing from text")?;
        let uri_end = uri_start
            .checked_add(uri.len())
            .context("muxr test URI position overflowed")?;
        assert_that!(
            links,
            each(
                predicate(|link: &PaneUrlLink| (uri_start..uri_end).contains(&link.cell()))
                    .with_description("points inside the URI", "points outside the URI")
            )
        );
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_url_wraps_at_row_edge_links_full_url() -> rootcause::Result<()> {
        let rows = self::rows(&["https://exam", "ple.com tail"])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        self::assert_linked_text(&rows, &links, "https://example.com", "https://example.com");
        assert_that!(
            links.iter().all(|link| !(link.row() == 1 && link.cell() == 7)),
            eq(true)
        );
        Ok(())
    }

    #[test]
    fn test_expand_visible_url_rows_when_changed_row_touches_wrapped_candidate_includes_connected_rows()
    -> rootcause::Result<()> {
        let rows = self::rows(&["https://exam", "ple.com tail ", "separate"])?;

        let expanded = self::expand_visible_url_rows(&rows, &self::soft_wraps(&rows), &[1]);

        assert_that!(expanded, eq(vec![0, 1]));
        Ok(())
    }

    #[test]
    fn test_expand_visible_url_rows_when_hard_lines_have_url_characters_keeps_changed_row_only() -> rootcause::Result<()>
    {
        let rows = self::rows(&["first", "second", "third"])?;
        let row_wraps = vec![RowWrap::EndsBeforeSoftWrap; rows.len()];

        let expanded = self::expand_visible_url_rows(&rows, &row_wraps, &[1]);

        assert_that!(expanded, eq(vec![1]));
        Ok(())
    }

    #[test]
    fn test_detect_visible_url_links_for_rows_when_unrelated_rows_exist_scans_only_dependency_rows()
    -> rootcause::Result<()> {
        let rows = self::rows(&["https://exam", "ple.com tail ", "https://other.example"])?;

        let links = self::detect_visible_url_links_for_rows(&rows, &self::soft_wraps(&rows), &[1])?;

        self::assert_linked_text(&rows, &links, "https://example.com", "https://example.com");
        assert_that!(links.iter().all(|link| link.row() < 2), eq(true));
        Ok(())
    }

    #[rstest]
    #[case::missing_host("http://")]
    #[case::malformed_host("https://:abc")]
    #[case::too_short_pane_edge_fragment("https://exam")]
    #[case::unsupported_scheme("ftp://example.com")]
    fn test_link_visible_urls_when_candidate_is_invalid_does_not_link(#[case] text: &str) -> rootcause::Result<()> {
        let rows = self::rows(&[text])?;

        let links = self::detect_visible_url_links(&rows, &self::soft_wraps(&rows))?;

        assert_that!(links, empty());
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_pane_fragments_are_linked_separately_does_not_cross_pane_boundary()
    -> rootcause::Result<()> {
        let left_rows = self::rows(&["https://exam"])?;
        let right_rows = self::rows(&["ple.com"])?;
        let left_pane_links = self::detect_visible_url_links(&left_rows, &self::soft_wraps(&left_rows))?;
        let right_pane_links = self::detect_visible_url_links(&right_rows, &self::soft_wraps(&right_rows))?;

        assert_that!(left_pane_links, empty());
        assert_that!(right_pane_links, empty());
        Ok(())
    }

    fn assert_linked_text(rows: &[RenderRowSpan], links: &[PaneUrlLink], text: &str, uri: &str) {
        let linked_text = links
            .iter()
            .map(|link| {
                rows.get(link.row())
                    .and_then(|row| row.cells().get(link.cell()))
                    .map(RenderCell::text)
                    .unwrap_or_default()
            })
            .collect::<String>();
        assert_that!(linked_text, eq(text));
        for link in links {
            assert_that!(link.hyperlink.uri(), eq(uri));
        }
    }

    fn rows(lines: &[&str]) -> rootcause::Result<Vec<RenderRowSpan>> {
        lines
            .iter()
            .enumerate()
            .map(|(row, line)| {
                let row = u16::try_from(row).context("muxr test row overflowed")?;
                RenderRowSpan::new(row, 0, line.chars().map(self::cell).collect())
            })
            .collect()
    }

    fn soft_wraps(rows: &[RenderRowSpan]) -> Vec<RowWrap> {
        let mut wraps = vec![RowWrap::EndsWithSoftWrap; rows.len()];
        if let Some(last) = wraps.last_mut() {
            *last = RowWrap::EndsBeforeSoftWrap;
        }
        wraps
    }

    fn cell(ch: char) -> RenderCell {
        RenderCell::narrow(ch.to_string(), RenderStyle::default())
    }
}
