use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;

use agg::AGENTS_PIPE;
use agg::ParseError;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::PipeMessage;
use zellij_tile::prelude::PipeSource;
use zellij_tile::prelude::TabInfo;

use crate::plugin::tbar::AGG_SYNC_PIPE;
use crate::plugin::tbar::StateSnapshotPayload;

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum PipeEventError {
    Parse(ParseError),
    UnknownMsgName(String),
}

impl Error for PipeEventError {}

impl Display for PipeEventError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::UnknownMsgName(name) => write!(f, "unknown message name {name:?}"),
        }
    }
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum PipeEvent {
    SyncRequest {
        requester_plugin_id: u32,
    },
    ActiveTab {
        active_tab_id: usize,
    },
    TabTopology {
        source_plugin_id: u32,
        tabs: Vec<TabInfo>,
    },
    StateSnapshot {
        source_plugin_id: u32,
        snapshot: Box<StateSnapshotPayload>,
    },
    Agent(AgentEventPayload),
}

impl PipeEvent {
    const fn source_plugin_id(msg: &PipeMessage) -> Option<u32> {
        match msg.source {
            PipeSource::Plugin(plugin_id) => Some(plugin_id),
            PipeSource::Cli(_) | PipeSource::Keybind => None,
        }
    }
}

impl TryFrom<&PipeMessage> for PipeEvent {
    type Error = PipeEventError;

    fn try_from(msg: &PipeMessage) -> Result<Self, Self::Error> {
        match msg.name.as_str() {
            AGG_SYNC_PIPE => match msg.args.get("type").map(String::as_str) {
                Some("sync_request") => {
                    let requester_plugin_id =
                        Self::source_plugin_id(msg).ok_or(PipeEventError::Parse(ParseError::Missing("source")))?;
                    Ok(Self::SyncRequest { requester_plugin_id })
                }
                Some("active_tab") => {
                    let active_tab_id = msg
                        .args
                        .get("tab_id")
                        .ok_or(PipeEventError::Parse(ParseError::Missing("tab_id")))
                        .and_then(|tab_id| {
                            tab_id.parse::<usize>().map_err(|_| {
                                PipeEventError::Parse(ParseError::Invalid {
                                    field: "tab_id",
                                    value: tab_id.clone(),
                                })
                            })
                        })?;
                    Ok(Self::ActiveTab { active_tab_id })
                }
                Some("tab_topology") => {
                    let source_plugin_id =
                        Self::source_plugin_id(msg).ok_or(PipeEventError::Parse(ParseError::Missing("source")))?;
                    let payload = msg
                        .payload
                        .as_deref()
                        .ok_or(PipeEventError::Parse(ParseError::Missing("tab_topology payload")))?;
                    let tabs = serde_json::from_str(payload).map_err(|error| {
                        PipeEventError::Parse(ParseError::Invalid {
                            field: "tab_topology payload",
                            value: error.to_string(),
                        })
                    })?;
                    Ok(Self::TabTopology { source_plugin_id, tabs })
                }
                Some("state_snapshot") => {
                    let source_plugin_id =
                        Self::source_plugin_id(msg).ok_or(PipeEventError::Parse(ParseError::Missing("source")))?;
                    let snapshot = StateSnapshotPayload::try_from(msg)?;
                    Ok(Self::StateSnapshot {
                        source_plugin_id,
                        snapshot: Box::new(snapshot),
                    })
                }
                Some(other) => Err(PipeEventError::UnknownMsgName(other.to_string())),
                None => Err(PipeEventError::Parse(ParseError::Missing("sync message type"))),
            },
            AGENTS_PIPE => {
                let pane_id = msg
                    .args
                    .get("pane_id")
                    .ok_or(PipeEventError::Parse(ParseError::Missing("pane_id")))?;
                let agent = msg
                    .args
                    .get("agent")
                    .ok_or(PipeEventError::Parse(ParseError::Missing("agent")))?;
                let payload = msg.payload.as_deref().unwrap_or("");
                let payload = AgentEventPayload::parse(pane_id, agent, payload).map_err(|e| {
                    PipeEventError::Parse(ParseError::Invalid {
                        field: "agent",
                        value: e.to_string(),
                    })
                })?;
                Ok(Self::Agent(payload))
            }
            _ => Err(PipeEventError::UnknownMsgName(msg.name.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::convert::TryFrom;
    use std::path::PathBuf;

    use agg::Cmd;
    use agg::GitStat;
    use agg::TabStateEntry;
    use rstest::rstest;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;
    use zellij_tile::prelude::TabInfo;

    use crate::plugin::tbar::PaneAgentSnapshot;
    use crate::plugin::tbar::events::*;

    #[rstest]
    #[case::sync_request(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args: BTreeMap::from([(String::from("type"), String::from("sync_request"))]),
            is_private: false,
        },
        PipeEvent::SyncRequest {
            requester_plugin_id: 7,
        }
    )]
    #[case::state_snapshot(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: Some(format!(
                "{}99\tcodex\tbusy\n",
                TabStateEntry {
                    tab_id: 17,
                    cwd: Some(PathBuf::from("/home/user/project")),
                    cmd: Cmd::Running("cargo test".to_string()),
                    indicator: agg::TabIndicator::NoAgent,
                    git_stat: GitStat {
                        insertions: 3,
                        deletions: 1,
                        new_files: 2,
                        is_worktree: true,
                        ..Default::default()
                    },
                }
            )),
            args: BTreeMap::from([
                (String::from("type"), String::from("state_snapshot")),
                (String::from("tab_id"), String::from("17")),
                (String::from("seq"), String::from("42")),
                (String::from("focused_pane_id"), String::from("99")),
                (String::from("cwd_pane_id"), String::from("99")),
            ]),
            is_private: false,
        },
        PipeEvent::StateSnapshot {
            source_plugin_id: 7,
            snapshot: Box::new(StateSnapshotPayload {
                tab_id: 17,
                seq: 42,
                focused_pane_id: Some(99),
                cwd_pane_id: Some(99),
                cwd: Some(PathBuf::from("/home/user/project")),
                cmd: Cmd::Running("cargo test".to_string()),
                indicator: agg::TabIndicator::NoAgent,
                git_stat: GitStat {
                    insertions: 3,
                    deletions: 1,
                    new_files: 2,
                    is_worktree: true,
                    ..Default::default()
                },
                pane_agents: vec![PaneAgentSnapshot {
                    pane_id: 99,
                    agent: Agent::Codex,
                    indicator: agg::TabIndicator::Busy,
                }],
            }),
        }
    )]
    #[case::active_tab(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args: BTreeMap::from([
                (String::from("type"), String::from("active_tab")),
                (String::from("tab_id"), String::from("17")),
            ]),
            is_private: false,
        },
        PipeEvent::ActiveTab { active_tab_id: 17 }
    )]
    #[case::tab_topology(
        {
            let tabs = vec![TabInfo {
                tab_id: 17,
                position: 1,
                name: "project".to_string(),
                active: true,
                ..Default::default()
            }];
            PipeMessage {
                source: PipeSource::Plugin(7),
                name: AGG_SYNC_PIPE.to_string(),
                payload: Some(serde_json::to_string(&tabs).unwrap()),
                args: BTreeMap::from([(String::from("type"), String::from("tab_topology"))]),
                is_private: false,
            }
        },
        PipeEvent::TabTopology {
            source_plugin_id: 7,
            tabs: vec![TabInfo {
                tab_id: 17,
                position: 1,
                name: "project".to_string(),
                active: true,
                ..Default::default()
            }],
        }
    )]
    #[case::agent(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGENTS_PIPE.to_string(),
            payload: Some(AgentEventKind::Busy.as_str().to_string()),
            args: BTreeMap::from([
                (String::from("pane_id"), String::from("99")),
                (String::from("agent"), String::from("codex")),
            ]),
            is_private: false,
        },
        PipeEvent::Agent(AgentEventPayload {
            pane_id: 99,
            agent: Agent::Codex,
            kind: AgentEventKind::Busy,
        })
    )]
    fn try_from_pipe_message_parses_supported_messages(#[case] msg: PipeMessage, #[case] expected: PipeEvent) {
        pretty_assertions::assert_eq!(PipeEvent::try_from(&msg), Ok(expected));
    }

    #[rstest]
    #[case::missing_sync_type(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args: BTreeMap::from([
                (String::from("tab_id"), String::from("17")),
                (String::from("seq"), String::from("42")),
            ]),
            is_private: false,
        },
        PipeEventError::Parse(ParseError::Missing("sync message type")),
    )]
    #[case::unknown_sync_type(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args: BTreeMap::from([(String::from("type"), String::from("unexpected"))]),
            is_private: false,
        },
        PipeEventError::UnknownMsgName("unexpected".to_string()),
    )]
    #[case::missing_source_for_sync_request(
        PipeMessage {
            source: PipeSource::Keybind,
            name: AGG_SYNC_PIPE.to_string(),
            payload: None,
            args: BTreeMap::from([(String::from("type"), String::from("sync_request"))]),
            is_private: false,
        },
        PipeEventError::Parse(ParseError::Missing("source")),
    )]
    #[case::missing_agent_field(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: AGENTS_PIPE.to_string(),
            payload: Some(AgentEventKind::Busy.as_str().to_string()),
            args: BTreeMap::from([(String::from("pane_id"), String::from("99"))]),
            is_private: false,
        },
        PipeEventError::Parse(ParseError::Missing("agent")),
    )]
    #[case::unknown_pipe_name(
        PipeMessage {
            source: PipeSource::Plugin(7),
            name: "other-pipe".to_string(),
            payload: None,
            args: BTreeMap::new(),
            is_private: false,
        },
        PipeEventError::UnknownMsgName("other-pipe".to_string()),
    )]
    fn try_from_pipe_message_reports_expected_errors(#[case] msg: PipeMessage, #[case] expected: PipeEventError) {
        pretty_assertions::assert_eq!(PipeEvent::try_from(&msg), Err(expected));
    }
}
