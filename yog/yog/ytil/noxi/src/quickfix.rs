//! Utilities for managing and displaying Nvim quickfix lists.

use core::fmt::Debug;

use color_eyre::eyre::Context;
use nvim_oxi::Array;
use nvim_oxi::api::opts::CmdOpts;
use nvim_oxi::api::types::CmdInfosBuilder;

use crate::dict;

/// Opens the quickfix window with the provided file and line number entries.
///
/// # Errors
/// - `setqflist` or `copen` command fails.
pub fn open<'a>(entries: impl IntoIterator<Item = (&'a str, i64)> + Debug) -> color_eyre::Result<()> {
    let mut qflist = vec![];
    for (filename, lnum) in entries {
        qflist.push(dict! {
            "filename": filename.to_string(),
            "lnum": lnum
        });
    }

    if qflist.is_empty() {
        return Ok(());
    }

    nvim_oxi::api::call_function::<_, i64>("setqflist", (Array::from_iter(qflist),))
        .wrap_err("error executing setqflist function")?;
    nvim_oxi::api::cmd(&CmdInfosBuilder::default().cmd("copen").build(), &CmdOpts::default())
        .wrap_err("error executing copen cmd")?;

    Ok(())
}
