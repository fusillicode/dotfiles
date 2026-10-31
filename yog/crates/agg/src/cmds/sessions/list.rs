use std::collections::HashMap;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;

use jiff::Timestamp;
use owo_colors::OwoColorize;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_agents::agent::Agent;
use ytil_agents::agent::session::Session;
use ytil_agents::agent::session::SessionKey;
use ytil_agents::agent::session_loader;
use ytil_ext::path_ext::PathExt;
use ytil_ext::string_ext::StringExt as _;

use crate::cmds::sessions::delete;

pub fn run(home_dir: &Path) -> rootcause::Result<()> {
    let sessions = load_sorted_sessions()?;

    if sessions.is_empty() {
        println!("No sessions");
        return Ok(());
    }

    let renderable_sessions = RenderableSession::from_sessions(sessions, home_dir);
    let searchable_sessions = SearchableSession::preload(renderable_sessions)?;
    let Some(selected) = ytil_tui::minimal_multi_select_with_raw_search(
        searchable_sessions,
        |entry| entry.renderable.to_string(),
        |entry| entry.renderable.preview(),
        |entry| Arc::clone(&entry.contents),
    )?
    else {
        println!("No sessions selected");
        return Ok(());
    };
    let selected: Vec<_> = selected.into_iter().map(|entry| entry.renderable).collect();

    let Some(op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        println!("No action selected");
        return Ok(());
    };

    match op {
        Op::Resume => ytil_tui::require_single(&selected, "sessions").and_then(launch_session),
        Op::Delete => delete::delete_selected_sessions(&selected, home_dir),
    }
}

pub fn run_json(args: &[String], home_dir: &Path) -> rootcause::Result<()> {
    let session_keys = match parse_json_session_keys(args) {
        Ok(session_keys) => session_keys,
        Err(error) => {
            eprintln!("{}", crate::cmds::Help::SessionsList.text());
            return Err(error);
        }
    };
    let sessions = load_sorted_sessions_by_key(&session_keys)?;
    let rows = RenderableSession::from_sessions(sessions, home_dir)
        .into_iter()
        .map(|session| JsonSession::new(&session))
        .collect::<rootcause::Result<Vec<_>>>()?;

    println!(
        "{}",
        serde_json::to_string(&rows).context("failed to serialize sessions")?
    );
    Ok(())
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub(super) struct RenderableSession {
    pub(super) session: Session,
    branch: Option<String>,
    home_dir: PathBuf,
}

impl RenderableSession {
    #[cfg(test)]
    pub(super) const fn for_test(session: Session, home_dir: PathBuf) -> Self {
        Self {
            session,
            branch: None,
            home_dir,
        }
    }

    fn from_sessions(sessions: Vec<Session>, home_dir: &Path) -> Vec<Self> {
        let mut timestamps_by_workspace = HashMap::<PathBuf, Vec<(usize, Timestamp)>>::new();
        for (index, session) in sessions.iter().enumerate() {
            timestamps_by_workspace
                .entry(session.workspace.clone())
                .or_default()
                .push((index, session.created_at));
        }

        let mut branches = vec![None; sessions.len()];
        for (workspace, timestamp_entries) in timestamps_by_workspace {
            let timestamps: Vec<Timestamp> = timestamp_entries.iter().map(|(_, timestamp)| *timestamp).collect();
            for ((index, _), branch) in timestamp_entries
                .into_iter()
                .zip(ytil_git::branch::get_at_many(&workspace, &timestamps))
            {
                if let Some(branch_slot) = branches.get_mut(index) {
                    *branch_slot = branch;
                }
            }
        }

        sessions
            .into_iter()
            .zip(branches)
            .map(|(session, branch)| Self {
                session,
                branch,
                home_dir: home_dir.to_path_buf(),
            })
            .collect()
    }

    fn can_resume(&self) -> bool {
        self.session.workspace.is_dir()
    }

    fn workspace_status(&self) -> &'static str {
        if self.can_resume() { "" } else { " [missing workspace]" }
    }

    fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    fn colored_agent_name(&self) -> String {
        match self.session.agent {
            Agent::Claude => self.session.agent.short_name().red().bold().to_string(),
            Agent::Codex => self.session.agent.short_name().green().bold().to_string(),
            Agent::Cursor => self.session.agent.short_name().bright_black().bold().to_string(),
            Agent::Gemini | Agent::Opencode => self.session.agent.short_name().bold().to_string(),
        }
    }

    fn plain_summary(&self) -> String {
        let path_label = self.session.workspace.short_path(&self.home_dir);
        let session_name = self.session.name.trim_end_at_with(42, None);
        let updated_label = self.session.updated_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let agent = self.session.agent.short_name();

        self.branch().map_or_else(
            || {
                format!(
                    "{agent} {path_label}{} {session_name} {updated_label} {created_label}",
                    self.workspace_status()
                )
            },
            |branch| {
                format!(
                    "{agent} {path_label} {branch}{} {session_name} {updated_label} {created_label}",
                    self.workspace_status()
                )
            },
        )
    }

    fn preview(&self) -> String {
        let first_prompt = self.session.name.as_str();
        let last_prompt = self.session.last_user_prompt.as_deref().unwrap_or("—");
        let updated = self.session.updated_at.strftime("%d/%m/%Y-%H:%M");
        let created = self.session.created_at.strftime("%d/%m/%Y-%H:%M");

        format!(
            "\n{}\n{}\n{}\n\n{}\n\n{}",
            self.session.id.white().bold(),
            updated.blue(),
            created.blue(),
            first_prompt.white(),
            last_prompt.white()
        )
    }
}

impl Display for RenderableSession {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let agent_name = self.colored_agent_name();
        let path_label = self.session.workspace.short_path(&self.home_dir);
        let updated_label = self.session.updated_at.strftime("%d/%m/%Y-%H:%M").to_string();
        let created_label = self.session.created_at.strftime("%d/%m/%Y-%H:%M").to_string();

        if let Some(branch) = self.branch() {
            write!(
                f,
                "{agent_name} {} {} {} {}",
                path_label.cyan().bold(),
                branch.dimmed().bold(),
                updated_label.blue(),
                created_label.blue(),
            )
        } else {
            write!(
                f,
                "{agent_name} {} {} {}",
                path_label.cyan().bold(),
                updated_label.blue(),
                created_label.blue()
            )
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct SearchableSession {
    renderable: RenderableSession,
    contents: Arc<[String]>,
}

impl SearchableSession {
    fn preload(sessions: Vec<RenderableSession>) -> rootcause::Result<Vec<Self>> {
        sessions
            .into_iter()
            .map(|renderable| {
                let path = &renderable.session.path;
                let paths = if renderable.session.agent == Agent::Cursor {
                    [path.join("meta.json"), path.join("prompt_history.json")]
                        .into_iter()
                        .filter(|path| path.is_file())
                        .collect()
                } else {
                    vec![path.clone()]
                };
                let contents = paths
                    .into_iter()
                    .map(|path| -> rootcause::Result<String> {
                        Ok(fs::read_to_string(&path)
                            .context("failed to preload session file for search")
                            .attach_with(|| format!("path={}", path.display()))?)
                    })
                    .collect::<rootcause::Result<Vec<_>>>()?;

                Ok(Self {
                    renderable,
                    contents: contents.into(),
                })
            })
            .collect()
    }
}

fn parse_json_session_keys(args: &[String]) -> rootcause::Result<Vec<SessionKey>> {
    let mut session_keys = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--session" => {
                let Some(key) = args.next() else {
                    return Err(report!("missing --session value"));
                };
                session_keys.push(key.parse()?);
            }
            unexpected => {
                return Err(report!("unknown agg sessions list --json arg").attach(format!("arg={unexpected}")));
            }
        }
    }
    if session_keys.is_empty() {
        return Err(report!("agg sessions list --json requires at least one --session"));
    }
    session_keys.sort();
    session_keys.dedup();
    Ok(session_keys)
}

fn load_sorted_sessions() -> rootcause::Result<Vec<Session>> {
    let mut sessions = Vec::new();
    sessions.extend(session_loader::load_sessions()?);
    sort_sessions(&mut sessions);
    Ok(sessions)
}

fn load_sorted_sessions_by_key(keys: &[SessionKey]) -> rootcause::Result<Vec<Session>> {
    let mut sessions = session_loader::load_sessions_by_key(keys)?;
    sort_sessions(&mut sessions);
    Ok(sessions)
}

fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[derive(Debug, Serialize)]
struct JsonSession {
    agent: &'static str,
    workspace: PathBuf,
    session_id: String,
    summary: String,
    display: String,
    search: String,
    updated_at: Timestamp,
    resume_program: String,
    resume_args: Vec<String>,
}

impl JsonSession {
    fn new(session: &RenderableSession) -> rootcause::Result<Self> {
        let display = session.plain_summary();
        let search = search_corpus(&display, &session.session.search_text);
        let (resume_program, resume_args) = session.session.build_resume_command()?;
        Ok(Self {
            agent: session.session.agent.name(),
            workspace: session.session.workspace.clone(),
            session_id: session.session.id.clone(),
            summary: session.session.name.clone(),
            display,
            search,
            updated_at: session.session.updated_at,
            resume_program: resume_program.to_string(),
            resume_args,
        })
    }
}

fn search_corpus(display_text: &str, hidden_search: &str) -> String {
    let visible_match_text = normalize_search(display_text);
    let hidden_search = normalize_search(hidden_search);
    if hidden_search.is_empty() || hidden_search == visible_match_text {
        visible_match_text
    } else {
        format!("{visible_match_text} {hidden_search}")
    }
}

fn normalize_search(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, EnumIter)]
enum Op {
    Resume,
    Delete,
}

impl Display for Op {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resume => write!(f, "{}", "Resume".green().bold()),
            Self::Delete => write!(f, "{}", "Delete".red().bold()),
        }
    }
}

fn launch_session(session: &RenderableSession) -> rootcause::Result<()> {
    if !session.can_resume() {
        return Err(report!("cannot resume session because its workspace is missing")
            .attach(format!("workspace={}", session.session.workspace.display()))
            .attach(format!("session_id={}", session.session.id)));
    }

    let session = &session.session;
    let (program, args) = session.build_resume_command()?;

    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(&session.workspace)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    #[cfg(unix)]
    {
        let error = cmd.exec();
        Err(report!("failed to exec agent CLI")
            .attach(format!("error={error}"))
            .attach(format!("agent={}", session.agent.name()))
            .attach(format!("workspace={}", session.workspace.display()))
            .attach(format!("session_id={}", session.id)))
    }

    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .context("failed to launch agent CLI")
            .attach_with(|| format!("agent={}", session.agent.name()))
            .attach_with(|| format!("workspace={}", session.workspace.display()))
            .attach_with(|| format!("session_id={}", session.id))?;

        if !status.success() {
            return Err(report!("agent CLI exited with non-zero status")
                .attach_with(|| format!("agent={}", session.agent.name()))
                .attach_with(|| format!("workspace={}", session.workspace.display()))
                .attach_with(|| format!("session_id={}", session.id))
                .attach_with(|| format!("status={status}")));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use tempfile::tempdir;
    use test_that::prelude::*;

    use super::*;

    #[test]
    fn test_preload_when_file_has_unindexed_fields_preserves_complete_raw_snapshot() {
        let dir = tempdir().expect("tempdir should be created");
        let path = dir.path().join("session.jsonl");
        let raw = format!(
            r#"{{"unknown_tool_output":"{}late-token  café\n"}}"#,
            "x".repeat(300_000)
        );
        fs::write(&path, &raw).expect("raw file should be written");
        let session = preload_fixture(Agent::Codex, path.clone(), dir.path());

        let actual = SearchableSession::preload(vec![session]).expect("complete file should preload");
        fs::write(&path, "changed after preload").expect("source file should change");

        assert_eq!(
            actual,
            vec![SearchableSession {
                renderable: preload_fixture(Agent::Codex, path, dir.path()),
                contents: vec![raw].into(),
            }]
        );
    }

    #[rstest::rstest]
    #[case(false)]
    #[case(true)]
    fn test_preload_when_cursor_uses_sidecar_files_keeps_available_files_separate(#[case] has_history: bool) {
        let dir = tempdir().expect("tempdir should be created");
        let path = dir.path().join("cursor-session");
        fs::create_dir(&path).expect("session directory should be created");
        let metadata = r#"{"unknown":"metadata token"}"#.to_owned();
        let history = r#"["history token"]"#.to_owned();
        fs::write(path.join("meta.json"), &metadata).expect("metadata should be written");
        let mut contents = vec![metadata];
        if has_history {
            fs::write(path.join("prompt_history.json"), &history).expect("history should be written");
            contents.push(history);
        }
        let session = preload_fixture(Agent::Cursor, path.clone(), dir.path());

        let actual = SearchableSession::preload(vec![session]).expect("sidecars should preload");

        assert_eq!(
            actual,
            vec![SearchableSession {
                renderable: preload_fixture(Agent::Cursor, path, dir.path()),
                contents: contents.into(),
            }]
        );
    }

    #[rstest::rstest]
    #[case::missing(None)]
    #[case::invalid_utf8(Some(b"\xff".as_slice()))]
    fn test_preload_when_file_cannot_be_loaded_returns_path_diagnostic(#[case] bytes: Option<&[u8]>) {
        let dir = tempdir().expect("tempdir should be created");
        let path = dir.path().join("unreadable-session.jsonl");
        if let Some(bytes) = bytes {
            fs::write(&path, bytes).expect("invalid text file should be written");
        }
        let session = preload_fixture(Agent::Codex, path, dir.path());

        let error = SearchableSession::preload(vec![session]).expect_err("loading errors must propagate");

        assert_eq!(
            error.format_current_context().to_string(),
            "failed to preload session file for search"
        );
        assert!(format!("{error:?}").contains("unreadable-session.jsonl"));
    }

    fn preload_fixture(agent: Agent, path: PathBuf, home: &Path) -> RenderableSession {
        let session = Session::new(
            agent,
            "session-id".to_owned(),
            home.join("workspace"),
            path,
            Some("summary-only".to_owned()),
            Timestamp::from_second(1).expect("valid timestamp"),
        );

        RenderableSession::for_test(session, home.to_path_buf())
    }

    #[test]
    fn test_search_corpus_matches_agg_visible_plus_hidden_filtering() {
        let display = "cx  ~/repo   branch   session name  09/05/2026-10:00";
        let hidden = "first user prompt\nassistant reply";

        let search = search_corpus(display, hidden);

        assert_that!(
            search,
            eq("cx ~/repo branch session name 09/05/2026-10:00 first user prompt assistant reply")
        );
    }

    #[test]
    fn test_json_session_renders_plain_agg_summary_and_resume_command() {
        let dir = tempdir().expect("tempdir should be created");
        let workspace = dir.path().join("repo");
        std::fs::create_dir_all(&workspace).expect("workspace should be created");
        let created_at = Timestamp::from_second(1_700_000_000).expect("test timestamp should be valid");
        let updated_at = Timestamp::from_second(1_700_000_100).expect("test timestamp should be valid");
        let session = Session {
            id: "session-id".to_string(),
            agent: Agent::Codex,
            name: "fix issue".to_string(),
            last_user_prompt: Some("finish the fix".to_string()),
            search_text: "hidden prompt".to_string(),
            workspace: workspace.clone(),
            path: dir.path().join("session.jsonl"),
            created_at,
            updated_at,
        };
        let renderable_sessions = RenderableSession::from_sessions(vec![session], dir.path());
        assert_that!(renderable_sessions.len(), eq(1));
        let renderable = &renderable_sessions[0];

        assert_renderable_session_agg_listing(renderable);
        assert_renderable_session_agg_json_listing(workspace, updated_at, renderable);

        assert_that!(
            renderable.preview(),
            all!(
                contains_substring("session-id"),
                contains_substring("fix issue"),
                contains_substring("finish the fix")
            )
        );

        let cursor_preview = RenderableSession {
            session: Session {
                agent: Agent::Cursor,
                last_user_prompt: None,
                ..renderable.session.clone()
            },
            branch: None,
            home_dir: renderable.home_dir.clone(),
        }
        .preview();
        assert_that!(cursor_preview, all!(contains_substring("—")));

        let prompt = "x".repeat(43);
        let full_prompt_preview = RenderableSession {
            session: Session {
                name: prompt.clone(),
                last_user_prompt: Some(prompt.clone()),
                ..renderable.session.clone()
            },
            branch: None,
            home_dir: renderable.home_dir.clone(),
        }
        .preview();

        assert_that!(full_prompt_preview, all!(contains_substring(prompt)));
    }

    #[test]
    fn test_parse_json_session_keys_requires_at_least_one_session_key() {
        assert_that!(
            (parse_json_session_keys(&[])).map(|_| ()),
            err(displays_as(contains_substring("requires at least one --session")))
        );
    }

    #[test]
    fn test_parse_json_session_keys_parses_and_dedupes_requested_session_keys() {
        assert_that!(
            parse_json_session_keys(&[
                String::from("--session"),
                String::from("codex:target"),
                String::from("--session"),
                String::from("codex:target"),
            ]),
            ok(eq([
                SessionKey::new(Agent::Codex, "target").expect("test value should be valid")
            ]))
        );
    }

    fn assert_renderable_session_agg_listing(renderable: &RenderableSession) {
        let display = RenderableSession {
            session: renderable.session.clone(),
            branch: Some("main".to_string()),
            home_dir: renderable.home_dir.clone(),
        }
        .to_string();
        assert_that!(
            display,
            all!(
                contains_substring("cx"),
                contains_substring("main"),
                contains_substring("14/11/2023-22:15"),
                contains_substring("14/11/2023-22:13"),
                contains_substring("~/repo"),
                not(contains_substring("fix issue"))
            )
        );
    }

    fn assert_renderable_session_agg_json_listing(
        workspace: std::path::PathBuf,
        updated_at: Timestamp,
        renderable: &RenderableSession,
    ) {
        assert_that!(
            JsonSession::new(renderable),
            ok(all!(
                result_of!(
                    |row: &JsonSession| row.display.as_str(),
                    starts_with("cx ~/repo fix issue")
                ),
                result_of!(
                    |row: &JsonSession| row.search.as_str(),
                    contains_substring("hidden prompt")
                ),
                result_of!(|row: &JsonSession| row.agent, eq("codex")),
                result_of!(|row: &JsonSession| &row.workspace, points_to(eq(workspace))),
                result_of!(|row: &JsonSession| row.session_id.as_str(), eq("session-id")),
                result_of!(|row: &JsonSession| row.summary.as_str(), eq("fix issue")),
                result_of!(|row: &JsonSession| row.updated_at, eq(updated_at)),
                result_of!(|row: &JsonSession| row.resume_program.as_str(), eq("codex")),
                result_of!(
                    |row: &JsonSession| row.resume_args.first().map(String::as_str),
                    eq(Some("resume"))
                ),
            ))
        );
    }
}
