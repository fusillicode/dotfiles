#![feature(exit_status_error)]

use std::process::Command;
use std::process::Stdio;

use color_eyre::owo_colors::OwoColorize as _;

use crate::pgpass::PgpassEntry;
use crate::pgpass::PgpassFile;
use crate::vault::VaultReadOutput;

mod nvim_dbee;
mod pgpass;
mod vault;

/// Manages `PostgreSQL` credentials from Vault and updates connection files.
///
/// After updating credentials, interactively prompts for confirmation before connecting via pgcli.
///
/// # Arguments
///
/// * `alias` - Database alias (optional, interactive selection if not provided)
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
        Option::default(),
    )?
    else {
        return Ok(());
    };

    println!(
        "\nLogging into Vault @ {}\n{}\n",
        std::env::var("VAULT_ADDR")?.bold(),
        "(be sure to have the VPN on!)".bold()
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

    println!(
        "{} credentials updated in {}",
        pgpass_entry.metadata.alias.green().bold(),
        pgpass_path.display()
    );
    println!(
        "{} credentials updated in {}",
        pgpass_entry.metadata.alias.green().bold(),
        nvim_dbee_conns_path.display()
    );
    // Cosmetic space in prompt.
    println!();

    if utils::sk::select_yes_or_no(format!("Connect to {}? ", pgpass_entry.metadata.alias))?.is_some_and(From::from) {
        let db_url = pgpass_entry.connection_params.db_url();
        println!(
            "\nConnecting to {} @\n\n{}\n",
            pgpass_entry.metadata.alias.bold(),
            db_url.bold()
        );

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

        eprintln!("{}", format!("pgcli {db_url} terminated by signal.").red().bold());
        std::process::exit(1);
    }

    Ok(())
}
