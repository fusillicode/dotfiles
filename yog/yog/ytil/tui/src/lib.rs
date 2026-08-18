//! Provide minimal TUI selection & prompt helpers built on [`skim`].
//!
//! Offer uniform, cancellable single / multi select prompts with fuzzy filtering and helpers
//! to derive a value from CLI args or fallback to an interactive selector.

#[cfg(not(target_arch = "wasm32"))]
pub use interactive::*;

#[cfg(not(target_arch = "wasm32"))]
pub mod git_branch;
#[cfg(not(target_arch = "wasm32"))]
mod interactive;
#[cfg(not(target_arch = "wasm32"))]
mod preview;
