//! Update Postgres credentials from Vault, rewrite pgpass & nvim-dbee, optionally launch pgcli.
//!
//! # Errors
//! - External command fails or file I/O fails.
//! - JSON serialization/deserialization fails.
//! - Environment variable missing or user interaction fails.
#![feature(exit_status_error)]

use std::process::Command;
use std::process::Stdio;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt;
use ytil_sys::cli::Args;

use crate::pgpass::PgpassEntry;
use crate::pgpass::PgpassFile;
use crate::vault::VaultReadOutput;

mod nvim_dbee;
mod pgpass;
mod vault;

/// Executes the `vault` CLI to read a secret as JSON.
///
/// # Errors
/// - Vault command fails or JSON deserialization fails.
fn exec_vault_read_cmd(vault_path: &str) -> rootcause::Result<VaultReadOutput> {
    let mut cmd = Command::new("vault");
    cmd.args(["read", vault_path, "--format=json"]);

    let cmd_stdout = &cmd.output()?.stdout;

    Ok(serde_json::from_slice(cmd_stdout)
        .context("error deserializing vault command output")
        .attach_with(|| {
            str::from_utf8(cmd_stdout).map_or_else(
                |error| format!("cmd={cmd:#?} error={error:?}"),
                |str_stdout| format!("cmd={cmd:#?} stdout={str_stdout:?}"),
            )
        })?)
}

/// Update Postgres credentials from Vault, rewrite pgpass & nvim-dbee, optionally launch pgcli.
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let pgpass_path = ytil_sys::dir::build_home_path(&[".pgpass"])?;
    let pgpass_content = std::fs::read_to_string(&pgpass_path)?;
    let pgpass_file = PgpassFile::parse(pgpass_content.as_str())?;

    let args = ytil_sys::cli::get();
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

    let nvim_dbee_conns_path = ytil_sys::dir::build_home_path(&[".local", "state", "nvim", "dbee", "conns.json"])?;
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

    println!(); // Cosmetic spacing.

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
