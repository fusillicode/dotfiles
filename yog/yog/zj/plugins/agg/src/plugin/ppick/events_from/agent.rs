use agg::AGENTS_PIPE;
use ytil_agents::agent::AgentEventPayload;
use zellij_tile::prelude::PipeMessage;

use crate::plugin::ppick::state::PpickEvent;

pub fn derive(msg: &PipeMessage) -> Vec<PpickEvent> {
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
        AgentEventPayload::parse(pane_id, agent, payload).inspect_err(|error| eprintln!("agg ppick: {error}"))
    else {
        return vec![];
    };

    vec![PpickEvent::AgentUpdated { event }]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;
    use ytil_agents::agent::Agent;
    use ytil_agents::agent::AgentEventKind;
    use ytil_agents::agent::AgentEventPayload;
    use zellij_tile::prelude::PipeMessage;
    use zellij_tile::prelude::PipeSource;

    use super::*;

    #[test]
    fn test_derive_agent_event_returns_update_for_agg_agent_pipe() {
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

        let events = derive(&msg);

        assert_eq!(
            events,
            vec![PpickEvent::AgentUpdated {
                event: AgentEventPayload {
                    pane_id: 42,
                    agent: Agent::Codex,
                    kind: AgentEventKind::Busy,
                },
            }]
        );
    }
}
