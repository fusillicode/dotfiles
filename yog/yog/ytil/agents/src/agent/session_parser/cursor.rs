use std::path::PathBuf;

use chrono::DateTime;
use rootcause::option_ext::OptionExt as _;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use serde::Deserialize;

use crate::agent::Agent;
use crate::agent::session::SearchTextBuilder;
use crate::agent::session::Session;

/// Parse Cursor session metadata into a session.
///
/// # Errors
/// Returns an error when the encoded metadata is invalid or contains an invalid timestamp.
pub fn parse(meta_hex: &str, workspace_dir: PathBuf) -> rootcause::Result<Session> {
    let meta_json = decode_hex_string(meta_hex)
        .context("failed to decode Cursor meta payload".to_owned())
        .attach(format!("meta_hex={meta_hex}"))?;
    let doc = serde_json::from_str::<CursorMeta>(&meta_json)
        .context("failed to parse Cursor session metadata".to_owned())
        .attach(format!("meta_json={meta_json}"))?;

    let created_at = DateTime::from_timestamp_millis(doc.created_at)
        .map(|datetime| datetime.to_utc())
        .context("Cursor createdAt is out of range".to_owned())
        .attach(format!("session_id={}", doc.agent_id))
        .attach(format!("created_at_ms={}", doc.created_at))?;

    Ok(Session::new(
        Agent::Cursor,
        doc.agent_id,
        workspace_dir,
        doc.name,
        created_at,
    ))
}

pub(crate) fn decode_hex_string(raw: &str) -> rootcause::Result<String> {
    let hex = raw.trim();

    if !hex.len().is_multiple_of(2) {
        return Err(report!("hex string has odd length").attach(format!("len={}", hex.len())));
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair).context("hex chunk is not utf8".to_owned())?;
        let byte = u8::from_str_radix(pair, 16).context("invalid hex byte".to_owned())?;
        bytes.push(byte);
    }

    Ok(String::from_utf8(bytes).context("decoded hex string is not utf8".to_owned())?)
}

pub fn build_search_text_from_strings(session_name: &str, strings_output: &str) -> String {
    let mut search_text = SearchTextBuilder::default();
    for line in strings_output.lines().filter_map(searchable_cursor_strings_line) {
        search_text.push(&line);
    }
    search_text.build(session_name)
}

pub fn extract_cursor_workspace_from_strings(
    strings_output: &str,
    known_workspaces: &[PathBuf],
    ignored_roots: &[PathBuf],
) -> Option<PathBuf> {
    let mut known_matches: Vec<PathBuf> = known_workspaces
        .iter()
        .filter(|workspace| workspace.to_str().is_some_and(|value| strings_output.contains(value)))
        .cloned()
        .collect();
    known_matches.sort_by_key(|workspace| std::cmp::Reverse(workspace.components().count()));
    if let Some(workspace) = known_matches.into_iter().next() {
        return Some(workspace);
    }

    for line in strings_output.lines() {
        for candidate in extract_absolute_path_candidates(line) {
            let Some(existing_path) = longest_existing_path(&candidate) else {
                continue;
            };
            let workspace_dir = if existing_path.is_dir() {
                existing_path
            } else if let Some(parent) = existing_path.parent() {
                parent.to_path_buf()
            } else {
                continue;
            };
            if ignored_roots.iter().any(|root| workspace_dir.starts_with(root)) {
                continue;
            }
            return Some(workspace_dir);
        }
    }

    None
}

#[derive(Debug, Deserialize)]
struct CursorMeta {
    #[serde(rename = "agentId")]
    agent_id: String,
    name: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: i64,
}

fn extract_absolute_path_candidates(line: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    candidates.extend(extract_prefixed_candidates(line, "file:///"));
    candidates.extend(extract_prefixed_candidates(line, "/"));
    candidates
}

fn searchable_cursor_strings_line(line: &str) -> Option<String> {
    let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = (!normalized.is_empty()).then_some(normalized)?;
    if normalized.len() < 8 {
        return None;
    }
    if !normalized.chars().any(char::is_alphabetic) || !normalized.chars().any(char::is_whitespace) {
        return None;
    }
    if normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    if !extract_absolute_path_candidates(&normalized).is_empty() {
        return None;
    }

    let lower = normalized.to_ascii_lowercase();
    if lower.contains("create table")
        || lower.contains("sqlite_")
        || lower.contains("indexsqlite_")
        || lower.starts_with("file:///")
    {
        return None;
    }

    Some(normalized)
}

fn extract_prefixed_candidates(line: &str, prefix: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut start = 0;
    while let Some(offset) = line[start..].find(prefix) {
        let absolute_start = start.saturating_add(offset);
        let suffix = &line[absolute_start..];
        let candidate: String = suffix.chars().take_while(|ch| is_path_char(*ch)).collect();
        if !candidate.is_empty() {
            candidates.push(candidate);
        }
        start = absolute_start.saturating_add(prefix.len());
    }
    candidates
}

const fn is_path_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '~')
}

fn longest_existing_path(candidate: &str) -> Option<PathBuf> {
    let normalized = candidate.strip_prefix("file://").unwrap_or(candidate);
    let mut path = PathBuf::from(normalized);

    while !path.exists() {
        if !path.pop() {
            return None;
        }
    }

    Some(path)
}

#[cfg(test)]
mod tests {

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_decodes_cursor_meta_hex_payload() {
        assert2::assert!(let Ok(decoded) = decode_hex_string("7b226e616d65223a225361666520526562617365227d"));
        pretty_assertions::assert_eq!(decoded, "{\"name\":\"Safe Rebase\"}");
    }

    #[test]
    fn test_parses_cursor_session_from_meta_json() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let meta_hex = "7b226167656e744964223a2266626364393632362d623065642d343739632d623838372d376132633264313531376636222c226e616d65223a225361666520526562617365222c22637265617465644174223a313737343837373733383031337d";
        assert2::assert!(let Ok(session) = parse(meta_hex, workspace.clone()));
        pretty_assertions::assert_eq!(session.agent, Agent::Cursor);
        pretty_assertions::assert_eq!(session.workspace, workspace);
        pretty_assertions::assert_eq!(session.name, "Safe Rebase");
    }

    #[test]
    fn test_extracts_cursor_workspace_from_known_workspaces_first() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("work").join("dotfiles");
        std::fs::create_dir_all(&workspace).unwrap();

        let strings_output = format!("file://{}/README.md\n{}\n", workspace.display(), workspace.display());
        let extracted = extract_cursor_workspace_from_strings(&strings_output, std::slice::from_ref(&workspace), &[]);
        pretty_assertions::assert_eq!(extracted, Some(workspace));
    }

    #[test]
    fn test_extracts_cursor_workspace_from_generic_path_candidates() {
        let tempdir = tempdir().unwrap();
        let workspace = tempdir.path().join("work").join("repo");
        let ignored = tempdir.path().join("home").join(".cursor");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::create_dir_all(&ignored).unwrap();

        let strings_output = format!("garbage file://{}/src/main.rs trailing", workspace.display());
        let extracted = extract_cursor_workspace_from_strings(&strings_output, &[], &[ignored]);
        pretty_assertions::assert_eq!(extracted, Some(workspace.join("src")));
    }

    #[test]
    fn test_build_search_text_from_strings_keeps_human_lines_and_filters_noise() {
        let strings_output = concat!(
            "CREATE TABLE blobs (id TEXT PRIMARY KEY, data BLOB);\n",
            "indexsqlite_autoindex_blobs_1blobs\n",
            "/Users/gianlu/data/dev/dotfiles/dotfiles/yog\n",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n",
            "user asked about stalled sync job\n",
            "user asked about stalled sync job\n",
            "assistant suggested retrying the worker\n"
        );

        let search_text = build_search_text_from_strings("Cursor Session", strings_output);

        pretty_assertions::assert_eq!(
            search_text,
            "Cursor Session user asked about stalled sync job assistant suggested retrying the worker"
        );
    }
}
