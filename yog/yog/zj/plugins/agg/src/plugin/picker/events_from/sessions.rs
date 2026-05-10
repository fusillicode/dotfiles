use crate::plugin::picker::state::PickerEvent;
use crate::plugin::picker::state::PickerState;
use crate::plugin::picker::state::SessionEntry;

pub fn derive(state: &PickerState, sessions: Vec<SessionEntry>) -> Vec<PickerEvent> {
    crate::plugin::picker::events_from::picker_event(state, PickerEvent::SessionsUpdated { sessions })
}

pub fn parse(stdout: &[u8]) -> serde_json::Result<Vec<SessionEntry>> {
    serde_json::from_slice(stdout)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_parse_parses_current_ags_json_contract() {
        let stdout = br#"[{"agent":"codex","workspace":"/tmp/repo","session_id":"abc","summary":"how to solve","display":"cx ~/repo fix","search":"hidden prompt","updated_at":"2026-05-09T10:00:00Z","resume_program":"codex","resume_args":["resume","abc"]}]"#;

        let entries = parse(stdout).unwrap();

        assert_eq!(
            entries,
            vec![SessionEntry {
                agent: "codex".to_string(),
                workspace: PathBuf::from("/tmp/repo"),
                session_id: "abc".to_string(),
                summary: Some("how to solve".to_string()),
                display: "cx ~/repo fix".to_string(),
                search: "hidden prompt".to_string(),
                updated_at: "2026-05-09T10:00:00Z".to_string(),
            }]
        );
    }
}
