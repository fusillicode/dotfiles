use std::fs::OpenOptions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::pgpass::PgpassEntry;

pub fn save_new_nvim_dbee_conns_file(updated_pg_pass_entry: &PgpassEntry, conns_path: &Path) -> color_eyre::Result<()> {
    let conns = get_update_conns(updated_pg_pass_entry, conns_path)?;

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

fn get_update_conns(updated_pg_pass_entry: &PgpassEntry, conns_path: &Path) -> color_eyre::Result<Vec<NvimDbeeConn>> {
    let updated_conn = NvimDbeeConn::from(updated_pg_pass_entry);

    // If the conn file does not exists returns just the updated conn
    if !conns_path.try_exists()? {
        return Ok(vec![updated_conn]);
    }

    let conns_file_content = std::fs::read_to_string(conns_path)?;
    // If the conn file exists but it's empty returns just the updated conn
    if conns_file_content.is_empty() {
        return Ok(vec![updated_conn]);
    }

    let mut conns: Vec<NvimDbeeConn> = serde_json::from_str(&conns_file_content)?;
    for conn in conns.iter_mut() {
        if conn.name == updated_pg_pass_entry.metadata.alias {
            conn.url = updated_pg_pass_entry.connection_params.db_url();
        }
    }
    // If the conn file exists and it is an empty vec just the updated conn
    if conns.is_empty() {
        conns.push(updated_conn)
    }
    Ok(conns)
}

#[derive(Deserialize, Serialize)]
struct NvimDbeeConn {
    id: String,
    name: String,
    url: String,
    r#type: String,
}

impl From<&PgpassEntry> for NvimDbeeConn {
    fn from(value: &PgpassEntry) -> Self {
        Self {
            id: value.metadata.alias.clone(),
            name: value.metadata.alias.clone(),
            url: value.connection_params.db_url(),
            r#type: "postgres".into(),
        }
    }
}
