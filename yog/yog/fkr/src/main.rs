//! Generate fake data strings from an enum façade over selected [`fake`] providers.
//!
//! # Errors
//! - Interactive selection UI fails.
//! - Writing the generated value to the clipboard fails.
#![feature(exit_status_error)]

use fkr::FkrOption;
use ytil_sys::cli::Args;

/// Entry point for the fake data generator CLI.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let Some(generated_value) = ytil_tui::minimal_select(FkrOption::to_vec())?.map(|fkr_opt| fkr_opt.gen_string())
    else {
        return Ok(());
    };

    println!("{generated_value}");

    if args.first().is_some_and(|arg| arg == "cp") {
        ytil_sys::file::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
