#![feature(exit_status_error)]
#![feature(error_generic_member_access)]

//! Utility library providing common functionality for yog tools.
//!
//! This library contains shared utilities and abstractions used across multiple
//! tools in the yog project. It provides modules for common operations like
//! command execution, system interactions, editor integration, and more.
//!
//! # Modules
//!
//! - `cmd`: Command execution utilities with error handling
//! - `editor`: Editor-specific functionality and abstractions
//! - `git`: Git repository operations and utilities
//! - `github`: GitHub API interactions and URL parsing
//! - `hx`: Helix editor integration utilities
//! - `sk`: skim (fuzzy finder) integration and item handling
//! - `system`: System-level operations like clipboard, file system
//! - `wezterm`: Wezterm terminal multiplexer integration
//!
//! # Design Principles
//!
//! - Cross-platform compatibility where possible
//! - Consistent error handling with `color_eyre`
//! - Efficient resource usage
//! - Clear and documented APIs
//! - Modular organization for easy testing and reuse

pub mod cmd;
pub mod editor;
pub mod git;
pub mod github;
pub mod hx;
pub mod sk;
pub mod system;
pub mod wezterm;
