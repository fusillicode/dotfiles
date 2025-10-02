//! Update Postgres creds from Vault, rewrite pgpass & nvim-dbee, optionally launch pgcli.
#![feature(exit_status_error)]

use std::process::Command;
use std::process::Stdio;

use color_eyre::eyre::Context;
use color_eyre::owo_colors::OwoColorize as _;

use crate::pgpass::PgpassEntry;
use crate::pgpass::PgpassFile;
use crate::vault::VaultReadOutput;

mod nvim_dbee;
mod pgpass;
mod vault;

/// Manage `PostgreSQL` credentials from Vault and update connection files.
///
/// After updating credentials, interactively prompts for confirmation before connecting via pgcli.
///
/// # Usage
/// ```bash
/// vpg # select alias interactively
/// vpg analytics # update creds for alias 'analytics'
/// ```
///
/// # Arguments
/// - `alias` Database alias (optional, interactive selection if not provided).
///
/// # Errors
/// - Executing one of the external commands (pgcli, vault) fails or returns a non-zero exit status.
/// - A filesystem operation (open/read/write/remove) fails.
/// - JSON serialization or deserialization fails.
/// - A required environment variable is missing or invalid Unicode.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let pgpass_path = ytil_system::build_home_path(&[".pgpass"])?;
    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let pgpass_file = PgpassFile::parse(pgpass_content.as_str())?;

    let args = ytil_system::get_args();
    let Some(mut pgpass_entry) = ytil_tui::get_item_from_cli_args_or_select(
        &args,
        |(idx, _)| *idx == 0,
        pgpass_file.entries,
        |alias: &str| Box::new(move |entry: &PgpassEntry| entry.metadata.alias == alias),
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
    let vault_read_output = exec_vault_read_cmd(&pgpass_entry.metadata.vault_path)?;

    pgpass_entry.connection_params.update(&vault_read_output.data);
    pgpass::save_new_pgpass_file(pgpass_file.idx_lines, &pgpass_entry.connection_params, &pgpass_path)?;

    let nvim_dbee_conns_path = ytil_system::build_home_path(&[".local", "state", "nvim", "dbee", "conns.json"])?;
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

    if Some(true) == ytil_tui::yes_no_select(&format!("Connect to {}? ", pgpass_entry.metadata.alias))? {
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

/// Executes the `vault` CLI to read a secret as JSON and deserialize it.
///
/// Runs `vault read <vault_path> --format=json` using [`std::process::Command`] and
/// deserializes the JSON standard output into a [`VaultReadOutput`].
///
/// # Arguments
/// - `vault_path` Path of the secret to read from Vault.
///
/// # Errors
/// - Launching or running the [`vault`] process fails (I/O error from [`Command`]).
/// - The command standard output cannot be deserialized into [`VaultReadOutput`] via [`serde_json`].
/// - The standard output is not valid UTF-8 when constructing the contextual error message.
fn exec_vault_read_cmd(vault_path: &str) -> color_eyre::Result<VaultReadOutput> {
    let mut cmd = Command::new("vault");
    cmd.args(["read", vault_path, "--format=json"]);

    let cmd_stdout = &cmd.output()?.stdout;

    serde_json::from_slice(cmd_stdout).with_context(|| {
        str::from_utf8(cmd_stdout).map_or_else(
            |error| format!("cmd stdout invalid utf-8 | cmd={cmd:#?} error={error:?}"),
            |str_stdout| format!("cannot build VaultReadOutput from vault cmd {cmd:#?} stdout {str_stdout:?}"),
        )
    })
}
