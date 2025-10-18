#![feature(exit_status_error)]

//! Generate fake data strings from an enum façade over selected [`fake`] providers.
//!
//! Provides a single variant enum ([`FkrOption`]) with a uniform `gen_string` method for
//! quick ad‑hoc values (UUIDs, emails, IPs, user agents) without pulling individual faker
//! types into every caller.
//!
//! # Arguments
//! - `cp` Optional flag to copy the generated value to clipboard.
//!
//! # Usage
//! ```bash
//! fkr # select a generator; prints value
//! fkr cp # select -> prints -> copies to clipboard
//! ```
//!
//! # Errors
//! - Interactive selection UI fails.
//! - Writing the generated value to the clipboard fails.

use fkr::FkrOption;

/// The actual `main` inner logic.
///
/// # Errors
/// - Interactive selection UI fails.
/// - Writing the generated value to the clipboard fails.
fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let Some(generated_value) = ytil_tui::minimal_select(FkrOption::to_vec())?.map(|fkr_opt| fkr_opt.gen_string())
    else {
        return Ok(());
    };

    println!("{generated_value}");

    if ytil_system::get_args().first().is_some_and(|arg| arg == "cp") {
        ytil_system::cp_to_system_clipboard(&mut generated_value.as_bytes())?;
    }

    Ok(())
}
