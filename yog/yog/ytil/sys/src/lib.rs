//! Provide cohesive system helpers: args, paths, symlinks, permissions, atomic copy, clipboard.
//!
//! Offer small utilities for CLI tools: joining thread handles, building home-relative paths,
//! manipulating filesystem entries (chmod, symlinks, atomic copy) and clipboard integration.

#![feature(exit_status_error)]

use std::process::Command;
use std::str::FromStr;
use std::thread::JoinHandle;

use color_eyre::eyre;
use color_eyre::eyre::Context;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
pub use pico_args;
use ytil_cmd::CmdExt as _;

pub mod cli;
pub mod dir;
pub mod file;
pub mod lsof;
pub mod rm;

/// Joins a thread handle and returns the result, handling join errors as [`eyre::Error`].
/// Awaits a `JoinHandle` and unwraps the inner `Result`.
///
/// # Errors
/// - The task panicked.
/// - The task returned an error.
pub fn join<T>(join_handle: JoinHandle<color_eyre::Result<T>>) -> Result<T, eyre::Error> {
    join_handle
        .join()
        .map_err(|err| eyre!("error joining handle | error={err:#?}"))?
}

/// Opens the given argument using the system's default opener.
///
/// # Arguments
/// - `arg` The argument to open (e.g., URL, or file path).
///
/// # Returns
/// Returns `Ok(())` if the command executes successfully.
///
/// # Errors
/// - The `open` command fails to execute.
/// - The `open` command exits with a non-zero status.
pub fn open(arg: &str) -> color_eyre::Result<()> {
    let cmd = "open";
    Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} {arg}"))
        .status()
        .wrap_err_with(|| eyre!("error running cmd | cmd={cmd:?} arg={arg:?}"))?
        .exit_ok()
        .wrap_err_with(|| eyre!("error cmd exit not ok | cmd={cmd:?} arg={arg:?}"))?;
    Ok(())
}

pub struct SysInfo {
    pub os: Os,
    pub arch: Arch,
}

impl SysInfo {
    /// Retrieves system information via `uname -mo`.
    ///
    /// # Errors
    /// - If `uname -mo` command fails.
    /// - If `uname -mo` output is unexpected.
    pub fn get() -> color_eyre::Result<Self> {
        Command::new("uname")
            .arg("-mo")
            .exec()
            .wrap_err_with(|| eyre!(r#"error running cmd | cmd="uname" arg="-mo""#))
            .and_then(|s| ytil_cmd::extract_success_output(&s))
            .and_then(|f| Self::from_str(f.as_str()))
    }
}

impl FromStr for SysInfo {
    type Err = color_eyre::eyre::Error;

    fn from_str(output: &str) -> Result<Self, Self::Err> {
        let mut os_arch = output.split_ascii_whitespace();

        let os = os_arch
            .next()
            .ok_or_else(|| eyre!("error missing os part in uname output | output={output:?}"))
            .and_then(Os::from_str)?;
        let arch = os_arch
            .next()
            .ok_or_else(|| eyre!("error missing arch part in uname output | output={output:?}"))
            .and_then(Arch::from_str)?;

        Ok(Self { os, arch })
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum Os {
    MacOs,
    Linux,
}

impl FromStr for Os {
    type Err = color_eyre::eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "darwin" => Ok(Self::MacOs),
            "linux" => Ok(Self::Linux),
            normalized_value => {
                bail!("error unknown normalized arch value | normalized_value={normalized_value:?} value={value:?} ")
            }
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum Arch {
    Arm,
    X86,
}

impl FromStr for Arch {
    type Err = color_eyre::eyre::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "x86_64" => Ok(Self::X86),
            "arm" => Ok(Self::Arm),
            normalized_value => {
                bail!(
                    "error unknown normalized arch value | value={value:?} normalized_value={normalized_value:?} value={value:?} "
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("x86_64", Arch::X86)]
    #[case("arm", Arch::Arm)]
    #[case("X86_64", Arch::X86)]
    #[case("ARM", Arch::Arm)]
    fn arch_from_str_when_valid_input_returns_expected_arch(#[case] input: &str, #[case] expected: Arch) {
        let result = Arch::from_str(input);
        assert2::let_assert!(Ok(arch) = result);
        pretty_assertions::assert_eq!(arch, expected);
    }

    #[test]
    fn arch_from_str_when_unknown_input_returns_error_with_message() {
        let result = Arch::from_str("unknown");
        assert2::let_assert!(Err(err) = result);
        assert!(err.to_string().contains("error unknown normalized arch value"));
    }

    #[rstest]
    #[case("darwin", Os::MacOs)]
    #[case("linux", Os::Linux)]
    #[case("DARWIN", Os::MacOs)]
    #[case("LINUX", Os::Linux)]
    fn os_from_str_when_valid_input_returns_expected_os(#[case] input: &str, #[case] expected: Os) {
        let result = Os::from_str(input);
        assert2::let_assert!(Ok(os) = result);
        pretty_assertions::assert_eq!(os, expected);
    }

    #[test]
    fn os_from_str_when_unknown_input_returns_error_with_message() {
        let result = Os::from_str("unknown");
        assert2::let_assert!(Err(err) = result);
        assert!(err.to_string().contains("error unknown normalized arch value"));
    }
}
