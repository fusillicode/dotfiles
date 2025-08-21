#![feature(exit_status_error)]

use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;

use crate::pgpass::PgpassEntry;
use crate::pgpass::PgpassFile;
use crate::vault::VaultReadOutput;

mod nvim_dbee;
mod pgpass;
mod vault;

/// Connects via pgcli to the DB matching the selected alias with refreshed Vault credentials.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let mut pgpass_path = PathBuf::from(std::env::var("HOME")?);
    pgpass_path.push(".pgpass");
    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let pgpass_file = PgpassFile::parse(pgpass_content.as_str())?;

    let args = utils::system::get_args();
    let Some(mut pgpass_entry) = utils::sk::get_item_from_cli_args_or_sk_select(
        &args,
        |(idx, _)| *idx == 0,
        pgpass_file.entries,
        |alias: &str| Box::new(move |entry: &PgpassEntry| entry.metadata.alias == alias),
        Default::default(),
    )?
    else {
        return Ok(());
    };

    println!(
        "\nLogging into Vault @ {}\n(be sure to have the VPN on!)",
        std::env::var("VAULT_ADDR")?
    );
    vault::log_into_vault_if_required()?;
    let vault_read_output: VaultReadOutput = serde_json::from_slice(
        &Command::new("vault")
            .args(["read", &pgpass_entry.metadata.vault_path, "--format=json"])
            .output()?
            .stdout,
    )?;

    pgpass_entry.connection_params.update(&vault_read_output.data);
    pgpass::save_new_pgpass_file(pgpass_file.idx_lines, &pgpass_entry.connection_params, &pgpass_path)?;
    let nvim_dbee_conns_path = PathBuf::from_str(&std::env::var("HOME")?)?.join(".local/state/nvim/dbee/conns.json");
    nvim_dbee::save_new_nvim_dbee_conns_file(&pgpass_entry, &nvim_dbee_conns_path)?;

    let db_url = pgpass_entry.connection_params.db_url();
    println!("\nConnecting to {} @\n\n{db_url}\n", pgpass_entry.metadata.alias);

    if let Some(psql_exit_code) = Command::new("pgcli")
        .arg(&db_url)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?
        .wait()?
        .code()
    {
        std::process::exit(psql_exit_code);
    }

    eprintln!("pgcli {db_url} terminated by signal.");
    std::process::exit(1);
}
