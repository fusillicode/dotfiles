#![feature(exit_status_error)]

use std::process::Command;
use std::process::Stdio;

use crate::pgpass::PgpassEntry;
use crate::pgpass::PgpassFile;
use crate::vault::VaultReadOutput;

mod nvim_dbee;
mod pgpass;
mod vault;

/// Manages PostgreSQL credentials from Vault and updates connection files.
///
/// # Arguments
///
/// * `alias` - Database alias (optional, interactive if not provided)
/// * `connect` - Optional flag to connect via pgcli
///
/// # Examples
///
/// ```bash
/// vpg
/// vpg mydb connect
/// ```
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let pgpass_path = utils::system::build_home_path(&[".pgpass"])?;
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
        "\nLogging into Vault @ {}\n(be sure to have the VPN on!)\n",
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

    let nvim_dbee_conns_path = utils::system::build_home_path(&[".local", "state", "nvim", "dbee", "conns.json"])?;
    nvim_dbee::save_new_nvim_dbee_conns_file(&pgpass_entry, &nvim_dbee_conns_path)?;

    println!("{} credentials updated in {pgpass_path:?}", pgpass_entry.metadata.alias);
    println!(
        "{} credentials updated in {nvim_dbee_conns_path:?}",
        pgpass_entry.metadata.alias
    );

    if args.get(1).is_some() {
        let db_url = pgpass_entry.connection_params.db_url();
        println!("Connecting to {} @\n\n{db_url}\n", pgpass_entry.metadata.alias);

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
    Ok(())
}
