use agg::AGENTS_PIPE;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::PipeMessage;

use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;

pub fn derive(state: &PickerState, msg: &PipeMessage) -> Vec<PickerEvent> {
    if msg.name != AGENTS_PIPE {
        return vec![];
    }

    let Some(pane_id) = msg.args.get("pane_id") else {
        return vec![];
    };
    let Some(agent) = msg.args.get("agent") else {
        return vec![];
    };
    let payload = msg.payload.as_deref().unwrap_or("");
    let Ok(event) =
        AgentEventPayload::parse(pane_id, agent, payload).inspect_err(|error| eprintln!("agg picker: {error}"))
    else {
        return vec![];
    };

    crate::plugin::picker::events_from::picker_event(state, PickerEvent::AgentUpdated { event })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::PaneInfo;
    use zellij_tile::prelude::PaneManifest;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;

    use super::*;

    #[test]
    fn test_derive_agent_event_returns_update_for_agg_agent_pipe() {
        let mut state = PickerState::default();
        let manifest = PaneManifest {
            panes: std::iter::once((
                0,
                vec![PaneInfo {
                    id: 42,
                    terminal_command: Some("codex".to_string()),
                    ..Default::default()
                }],
            ))
            .collect(),
        };
        let _ = state.update_panes(&manifest, |_| None, |_| Some(vec![String::from("codex")]));
        let msg = PipeMessage {
            source: PipeSource::Keybind,
            name: AGENTS_PIPE.to_string(),
            payload: Some(AgentEventKind::Busy.as_str().to_string()),
            args: BTreeMap::from([
                (String::from("pane_id"), String::from("42")),
                (String::from("agent"), String::from("codex")),
            ]),
            is_private: false,
        };

        let events = derive(&state, &msg);

        assert_eq!(
            events,
            vec![PickerEvent::AgentUpdated {
                event: AgentEventPayload {
                    pane_id: 42,
                    agent: Agent::Codex,
                    kind: AgentEventKind::Busy,
                },
            }]
        );
    }
}
