use std::fs::OpenOptions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::pgpass::PgpassEntry;

pub fn save_new_nvim_dbee_conns_file(updated_pg_pass_entry: &PgpassEntry, conns_path: &Path) -> color_eyre::Result<()> {
    let conns = if !conns_path.exists() {
        vec![NvimDbeeConn {
            id: updated_pg_pass_entry.metadata.alias.clone(),
            name: updated_pg_pass_entry.metadata.alias.clone(),
            url: updated_pg_pass_entry.connection_params.db_url(),
            r#type: "postgres".into(),
        }]
    } else {
        let conns_file_content = std::fs::read_to_string(conns_path)?;
        if conns_file_content.is_empty() {
            vec![NvimDbeeConn {
                id: updated_pg_pass_entry.metadata.alias.clone(),
                name: updated_pg_pass_entry.metadata.alias.clone(),
                url: updated_pg_pass_entry.connection_params.db_url(),
                r#type: "postgres".into(),
            }]
        } else {
            let conns: Vec<NvimDbeeConn> = serde_json::from_str(&conns_file_content)?;
            let mut conns = if conns.is_empty() {
                vec![NvimDbeeConn {
                    id: updated_pg_pass_entry.metadata.alias.clone(),
                    name: updated_pg_pass_entry.metadata.alias.clone(),
                    url: updated_pg_pass_entry.connection_params.db_url(),
                    r#type: "postgres".into(),
                }]
            } else {
                conns
            };
            for conn in conns.iter_mut() {
                if conn.name == updated_pg_pass_entry.metadata.alias {
                    conn.url = updated_pg_pass_entry.connection_params.db_url();
                }
            }
            conns
        }
    };

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

#[derive(Deserialize, Serialize)]
struct NvimDbeeConn {
    id: String,
    name: String,
    url: String,
    r#type: String,
}
