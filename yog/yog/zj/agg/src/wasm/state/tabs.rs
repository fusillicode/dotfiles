use std::collections::HashSet;

use zellij_tile::prelude::TabInfo;

use super::current_tab::CurrentTab;

pub fn topology_changed(x: &[TabInfo], y: &[TabInfo]) -> bool {
    if x.len() != y.len() {
        return true;
    }

    x.iter()
        .zip(y.iter())
        .any(|(left, right)| left.tab_id != right.tab_id || left.position != right.position)
}

pub fn detect_remapped_tab_id(
    current_tab: Option<&CurrentTab>,
    prev_tabs: &[TabInfo],
    new_tabs: &[TabInfo],
) -> Option<usize> {
    let current_tab = current_tab?;
    if new_tabs.iter().any(|tab| tab.tab_id == current_tab.tab_id) {
        return None;
    }

    let prev_ids: HashSet<usize> = prev_tabs.iter().map(|tab| tab.tab_id).collect();
    let next_ids: HashSet<usize> = new_tabs.iter().map(|tab| tab.tab_id).collect();
    let removed_ids: HashSet<usize> = prev_ids.difference(&next_ids).copied().collect();
    if !removed_ids.contains(&current_tab.tab_id) {
        return None;
    }

    let mut added_tabs: Vec<&TabInfo> = new_tabs.iter().filter(|tab| !prev_ids.contains(&tab.tab_id)).collect();
    if added_tabs.is_empty() {
        return None;
    }
    if added_tabs.len() > 1
        && let Some(prev_current_tab) = prev_tabs.iter().find(|tab| tab.tab_id == current_tab.tab_id)
    {
        let matching_names: Vec<&TabInfo> = added_tabs
            .iter()
            .copied()
            .filter(|tab| tab.name == prev_current_tab.name)
            .collect();
        if matching_names.len() == 1 {
            added_tabs = matching_names;
        }
    }

    if added_tabs.len() == 1 {
        added_tabs.first().map(|tab| tab.tab_id)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::wasm::state::test_support::*;

    #[test]
    fn test_detect_remapped_tab_id_prefers_matching_name() {
        let current_tab = CurrentTab::new(10);
        let prev_tabs = vec![tab_with_name(10, 0, "agent")];
        let new_tabs = vec![tab_with_name(20, 0, "other"), tab_with_name(30, 1, "agent")];

        assert_eq!(
            detect_remapped_tab_id(Some(&current_tab), &prev_tabs, &new_tabs),
            Some(30)
        );
    }
}
