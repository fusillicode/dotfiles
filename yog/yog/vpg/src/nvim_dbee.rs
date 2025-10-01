use std::fs::OpenOptions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::pgpass::PgpassEntry;

/// Saves or updates the `nvim-dbee` connections file with the provided [`PgpassEntry`], setting secure permissions.
///
/// # Errors
/// Returns an error if:
/// - A filesystem operation (open/read/write/remove) fails.
/// - JSON serialization or deserialization fails.
pub fn save_new_nvim_dbee_conns_file(updated_pg_pass_entry: &PgpassEntry, conns_path: &Path) -> color_eyre::Result<()> {
    let conns = get_updated_conns(updated_pg_pass_entry, conns_path)?;

    let mut tmp_path = PathBuf::from(conns_path);
    tmp_path.set_file_name("conns.json.tmp");
    std::fs::write(&tmp_path, serde_json::to_string(&conns)?)?;
    std::fs::rename(&tmp_path, conns_path)?;

    let file = OpenOptions::new().read(true).open(conns_path)?;
    let mut permissions = file.metadata()?.permissions();
    permissions.set_mode(0o600);
    file.set_permissions(permissions)?;

    Ok(())
}

/// Get updated conns.
///
/// # Errors
/// Returns an error if:
/// - A filesystem operation (open/read/write/remove) fails.
/// - JSON serialization or deserialization fails.
fn get_updated_conns(updated_pg_pass_entry: &PgpassEntry, conns_path: &Path) -> color_eyre::Result<Vec<NvimDbeeConn>> {
    let updated_conn = NvimDbeeConn::from(updated_pg_pass_entry);

    // Returns just the updated conn if the conns file does not exist.
    if !conns_path.try_exists()? {
        return Ok(vec![updated_conn]);
    }

    let conns_file_content = std::fs::read_to_string(conns_path)?;
    // Returns just the updated conn if the conns file exists but it's empty.
    if conns_file_content.is_empty() {
        return Ok(vec![updated_conn]);
    }

    // Returns the already present conns with the updated one added in case it was missing.
    let mut conns: Vec<NvimDbeeConn> = serde_json::from_str(&conns_file_content)?;
    let mut seen = false;
    for conn in &mut conns {
        if conn.name == updated_conn.name {
            conn.url = updated_pg_pass_entry.connection_params.db_url();
            seen = true;
        }
    }
    if !seen {
        conns.push(updated_conn);
    }
    Ok(conns)
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct NvimDbeeConn {
    id: String,
    name: String,
    r#type: String,
    url: String,
}

impl From<&PgpassEntry> for NvimDbeeConn {
    fn from(value: &PgpassEntry) -> Self {
        Self {
            id: value.metadata.alias.clone(),
            name: value.metadata.alias.clone(),
            url: value.connection_params.db_url(),
            r#type: "postgres".to_string(),
        }
    }
}
