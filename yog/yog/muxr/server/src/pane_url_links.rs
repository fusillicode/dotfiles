use muxr_core::RenderHyperlink;
use muxr_core::RenderRowSpan;
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
}

pub fn detect_visible_url_links(rows: &[RenderRowSpan]) -> rootcause::Result<Vec<PaneUrlLink>> {
    let mut links = Vec::new();
    let mut linked_cells = rows
        .iter()
        .map(|row| vec![false; row.cells().len()])
        .collect::<Vec<_>>();
    for (row, span) in rows.iter().enumerate() {
        for cell in 0..span.cells().len() {
            let position = CellPosition { cell, row };
            if self::cell_is_linked(&linked_cells, position) {
                continue;
            }
            let Some(candidate) = self::url_candidate_at(rows, position) else {
                continue;
            };
            let Some(uri) = self::valid_url_prefix(&candidate) else {
                continue;
            };
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

fn cell_is_linked(linked_cells: &[Vec<bool>], position: CellPosition) -> bool {
    linked_cells
        .get(position.row)
        .and_then(|row| row.get(position.cell))
        .copied()
        .unwrap_or(false)
}

fn mark_cell_linked(linked_cells: &mut [Vec<bool>], position: CellPosition) -> rootcause::Result<()> {
    let Some(cell) = linked_cells
        .get_mut(position.row)
        .and_then(|row| row.get_mut(position.cell))
    else {
        return Err(report!("muxr pane url link position is outside visible rows"));
    };
    *cell = true;
    Ok(())
}

fn url_candidate_at(rows: &[RenderRowSpan], position: CellPosition) -> Option<UrlCandidate> {
    let scheme_len = self::scheme_len_at(rows, position)?;
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
        if !self::is_url_char(ch) {
            break UrlCandidateEnd::Delimiter;
        }
        positions.push(position);
        text.push(ch);
        current = self::next_position(rows, position);
    };

    if text.len() <= scheme_len {
        return None;
    }

    Some(UrlCandidate { end, positions, text })
}

fn scheme_len_at(rows: &[RenderRowSpan], position: CellPosition) -> Option<usize> {
    [HTTPS_SCHEME, HTTP_SCHEME]
        .into_iter()
        .find(|scheme| self::matches_scheme_at(rows, position, scheme))
        .map(str::len)
}

fn matches_scheme_at(rows: &[RenderRowSpan], position: CellPosition, scheme: &str) -> bool {
    let mut current = Some(position);
    for expected in scheme.chars() {
        let Some(position) = current else {
            return false;
        };
        if self::cell_ascii_char(rows, position) != Some(expected) {
            return false;
        }
        current = self::next_position(rows, position);
    }
    true
}

fn valid_url_prefix(candidate: &UrlCandidate) -> Option<String> {
    let trimmed = candidate.text.trim_end_matches(self::is_trailing_punctuation);
    let url = Url::parse(trimmed).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    if self::ambiguous_visible_edge_fragment(candidate, trimmed, &url) {
        return None;
    }
    Some(trimmed.to_owned())
}

fn ambiguous_visible_edge_fragment(candidate: &UrlCandidate, text: &str, url: &Url) -> bool {
    if candidate.end != UrlCandidateEnd::VisibleEdge {
        return false;
    }
    // `url` accepts bare single-label hosts such as `https://exam`; at the visible edge those are often just the
    // first row of a wrapped URL, so reject only that ambiguous bare-authority case.
    if self::url_has_host_boundary_signal(url) || !self::url_text_is_bare_authority(text, url) {
        return false;
    }
    true
}

fn url_text_is_bare_authority(text: &str, url: &Url) -> bool {
    let Some(authority_start) = url.scheme().len().checked_add("://".len()) else {
        return false;
    };
    let Some(after_scheme) = text.get(authority_start..) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return true;
    };
    after_scheme == host
}

fn url_has_host_boundary_signal(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host == "localhost" || host.contains('.') || url.port().is_some()
}

const fn is_trailing_punctuation(ch: char) -> bool {
    matches!(ch, '.' | ',' | ';' | '!' | '?')
}

fn next_position(rows: &[RenderRowSpan], position: CellPosition) -> Option<CellPosition> {
    let next_cell = position.cell.checked_add(1)?;
    if next_cell < rows.get(position.row)?.cells().len() {
        return Some(CellPosition {
            row: position.row,
            cell: next_cell,
        });
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

const fn is_url_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
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
}

#[cfg(test)]
mod tests {
    use muxr_core::RenderCell;
    use muxr_core::RenderStyle;
    use rootcause::prelude::ResultExt;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::http("http://example.com")]
    #[case::https("https://example.com")]
    #[case::localhost("http://localhost:3000")]
    #[case::explicit_port_at_edge("http://grafana:3000")]
    fn test_link_visible_urls_when_plain_url_is_visible_links_url_cells(#[case] uri: &str) -> rootcause::Result<()> {
        let rows = self::rows(&[&format!("go {uri}")])?;

        let links = self::detect_visible_url_links(&rows)?;

        self::assert_linked_text(&rows, &links, uri, uri);
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_single_label_host_has_delimiter_links_url() -> rootcause::Result<()> {
        let rows = self::rows(&["go http://grafana now"])?;

        let links = self::detect_visible_url_links(&rows)?;

        self::assert_linked_text(&rows, &links, "http://grafana", "http://grafana");
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_single_label_host_has_explicit_path_at_edge_links_url() -> rootcause::Result<()> {
        let rows = self::rows(&["http://grafana/"])?;

        let links = self::detect_visible_url_links(&rows)?;

        self::assert_linked_text(&rows, &links, "http://grafana/", "http://grafana/");
        Ok(())
    }

    #[rstest]
    #[case::period("https://example.com.", "https://example.com")]
    #[case::comma("https://example.com,", "https://example.com")]
    #[case::semicolon("https://example.com;", "https://example.com")]
    #[case::bang("https://example.com!", "https://example.com")]
    #[case::question("https://example.com?", "https://example.com")]
    fn test_link_visible_urls_when_url_has_trailing_punctuation_trims_link_target(
        #[case] text: &str,
        #[case] uri: &str,
    ) -> rootcause::Result<()> {
        let rows = self::rows(&[text])?;

        let links = self::detect_visible_url_links(&rows)?;

        self::assert_linked_text(&rows, &links, uri, uri);
        assert2::assert!(links.iter().all(|link| link.cell() < uri.len()));
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_url_wraps_at_row_edge_links_full_url() -> rootcause::Result<()> {
        let rows = self::rows(&["https://exam", "ple.com tail"])?;

        let links = self::detect_visible_url_links(&rows)?;

        self::assert_linked_text(&rows, &links, "https://example.com", "https://example.com");
        assert2::assert!(links.iter().all(|link| !(link.row() == 1 && link.cell() == 7)));
        Ok(())
    }

    #[rstest]
    #[case::missing_host("http://")]
    #[case::malformed_host("https://:abc")]
    #[case::too_short_pane_edge_fragment("https://exam")]
    #[case::unsupported_scheme("ftp://example.com")]
    fn test_link_visible_urls_when_candidate_is_invalid_does_not_link(#[case] text: &str) -> rootcause::Result<()> {
        let rows = self::rows(&[text])?;

        let links = self::detect_visible_url_links(&rows)?;

        assert2::assert!(links.is_empty());
        Ok(())
    }

    #[test]
    fn test_link_visible_urls_when_pane_fragments_are_linked_separately_does_not_cross_pane_boundary()
    -> rootcause::Result<()> {
        let left_pane_links = self::detect_visible_url_links(&self::rows(&["https://exam"])?)?;
        let right_pane_links = self::detect_visible_url_links(&self::rows(&["ple.com"])?)?;

        assert2::assert!(left_pane_links.is_empty());
        assert2::assert!(right_pane_links.is_empty());
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
        pretty_assertions::assert_eq!(linked_text, text);
        for link in links {
            pretty_assertions::assert_eq!(link.hyperlink.uri(), uri);
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

    fn cell(ch: char) -> RenderCell {
        RenderCell::narrow(ch.to_string(), RenderStyle::default())
    }
}
