use crate::plugin::tbar::Event;
use crate::plugin::tbar::StateSnapshotPayload;
use crate::plugin::tbar::TbarState;

pub fn derive(state: &TbarState, source_plugin_id: u32, snapshot: &StateSnapshotPayload) -> Vec<Event> {
    if source_plugin_id == state.plugin_id
        || state.current_tab_id() == Some(snapshot.tab_id)
        || !state.all_tabs.iter().any(|tab| tab.tab_id == snapshot.tab_id)
        || state
            .other_tabs
            .get(&source_plugin_id)
            .is_some_and(|remote| snapshot.seq <= remote.seq)
    {
        return vec![];
    }

    let evict_ids = state
        .other_tabs
        .iter()
        .filter(|&(plugin_id, remote)| *plugin_id != source_plugin_id && remote.tab_id == snapshot.tab_id)
        .map(|(&plugin_id, _)| plugin_id)
        .collect();

    vec![Event::RemoteTabUpdated {
        source_plugin_id,
        snapshot: snapshot.clone(),
        evict_ids,
    }]
}
