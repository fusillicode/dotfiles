//! Diagnostic processing utilities for LSP diagnostics.
//!
//! This module provides functionality to filter, format, and sort LSP diagnostics
//! received from language servers in Nvim.

use nvim_oxi::Dictionary;

use crate::dict;
use crate::fn_from;

mod filter;
mod filters;
mod formatter;
mod sorter;

/// [`Dictionary`] of diagnostic processing helpers.
pub fn dict() -> Dictionary {
    dict! {
        "format": fn_from!(formatter::format),
        "sort": fn_from!(sorter::sort),
        "filter": fn_from!(filter::filter),
    }
}
