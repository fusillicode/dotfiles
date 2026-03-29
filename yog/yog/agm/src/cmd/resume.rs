use agm_core::Agent;
use strum::IntoEnumIterator;

#[allow(dead_code)]
struct Session {
    agent: Agent,
    name: String,
    created_at: String,
}

pub fn run() -> rootcause::Result<()> {
    // TODO instead of showing an agent selection get all the possible sessions across all the
    // Agents (the enum implement EnumIter) and display them
    let Some(selected) = ytil_tui::minimal_select::<Agent>(Agent::iter().collect())? else {
        println!("Nothing selected");
        return Ok(());
    };
    // TODO: when a session is selected proceed by opening the related agent
    println!("Selected {selected}");
    Ok(())
}
