#![feature(exit_status_error)]

use std::process::Command;

use color_eyre::owo_colors::OwoColorize;
use ytil_cmd::CmdExt as _;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_system::get_args();
    let path = args.first().unwrap().clone();

    let lints_handles = [
        (
            "cargo-machete",
            std::thread::spawn({
                let path = path.clone();
                move || {
                    // Run cargo-machete directly. Invoking through `cargo machete` causes the subcommand
                    // name ("machete") to be forwarded as a spurious path argument, triggering false path
                    // scanning attempts (observed: it tried to scan a directory literally named "machete").
                    Command::new("cargo-machete").args(["--with-metadata", &path]).exec()
                }
            }),
        ),
        (
            "cargo-sort",
            std::thread::spawn({
                let path = path.clone();
                move || {
                    Command::new("cargo-sort")
                        .args(["--workspace", "--check", "--check-format"])
                        .current_dir(path)
                        .exec()
                }
            }),
        ),
    ];

    let mut errors = false;
    for (lint, lint_handle) in lints_handles {
        match lint_handle.join() {
            Ok(Ok(output)) => {
                println!(
                    "{} {} {}",
                    format!("Success {lint}").green().bold(),
                    format!("status={:?}", output.status.code()).white().bold(),
                    format!(
                        "stdout={:?}",
                        str::from_utf8(&strip_ansi_escapes::strip(&output.stdout))
                    )
                    .white()
                    .bold()
                );
            }
            Ok(Err(error)) => {
                eprintln!(
                    "{} {}",
                    format!("Error {lint}").red().bold(),
                    format!("error={error}").white().bold()
                );
                errors = true;
            }
            Err(error) => {
                eprintln!("{}", format!("JoinHandle error={error:?}").red().bold());
                errors = true;
            }
        }
    }

    if errors {
        std::process::exit(1);
    }

    Ok(())
}
