#![feature(exit_status_error)]
#![feature(error_generic_member_access)]

//! Shared utility library for yog tools.
//!
//! # Modules
//!
//! - `cmd`: Command execution utilities
//! - `editor`: Editor functionality
//! - `git`: Git operations
//! - `github`: GitHub API interactions
//! - `hx`: Helix editor integration
//! - `system`: System operations
//! - `wezterm`: Wezterm integration

pub mod cmd;
pub mod editor;
pub mod git;
pub mod github;
pub mod hx;
pub mod inquire;
pub mod system;
pub mod wezterm;
