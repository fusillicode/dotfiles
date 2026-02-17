//! System helpers: args, paths, symlinks, permissions, clipboard.
#![feature(exit_status_error)]

use std::process::Command;
use std::str::FromStr;
use std::thread::JoinHandle;

pub use pico_args;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_cmd::CmdExt as _;

pub mod cli;
pub mod dir;
pub mod file;
pub mod lsof;
pub mod rm;

/// Joins a thread handle and returns the result.
///
/// # Errors
/// - Task panicked or returned an error.
pub fn join<T>(join_handle: JoinHandle<rootcause::Result<T>>) -> Result<T, rootcause::Report> {
    join_handle
        .join()
        .map_err(|err| report!("error joining handle").attach(format!("error={err:#?}")))?
}

/// Opens the given argument using the system's default app (`open` on macOS).
///
/// # Errors
/// - `open` command fails.
pub fn open(arg: &str) -> rootcause::Result<()> {
    let cmd = "open";
    Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} '{arg}'"))
        .status()
        .context("error running cmd")
        .attach_with(|| format!("cmd={cmd:?} arg={arg:?}"))?
        .exit_ok()
        .context("error cmd exit not ok")
        .attach_with(|| format!("cmd={cmd:?} arg={arg:?}"))?;
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
    pub fn get() -> rootcause::Result<Self> {
        let output = Command::new("uname")
            .arg("-mo")
            .exec()
            .context("error running cmd")
            .attach(r#"cmd="uname" arg="-mo""#)?;
        let s = ytil_cmd::extract_success_output(&output)?;
        Self::from_str(s.as_str())
    }
}

impl FromStr for SysInfo {
    type Err = rootcause::Report;

    fn from_str(output: &str) -> Result<Self, Self::Err> {
        let mut os_arch = output.split_ascii_whitespace();

        let os = os_arch
            .next()
            .ok_or_else(|| report!("error missing os part in uname output"))
            .attach_with(|| format!("output={output:?}"))
            .and_then(Os::from_str)?;
        let arch = os_arch
            .next()
            .ok_or_else(|| report!("error missing arch part in uname output"))
            .attach_with(|| format!("output={output:?}"))
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
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "darwin" => Ok(Self::MacOs),
            "linux" => Ok(Self::Linux),
            normalized_value => Err(report!("error unknown normalized os value")
                .attach(format!("normalized_value={normalized_value:?} value={value:?}"))),
        }
    }
}

#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
pub enum Arch {
    Arm,
    X86,
}

impl FromStr for Arch {
    type Err = rootcause::Report;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "x86_64" => Ok(Self::X86),
            "arm64" => Ok(Self::Arm),
            normalized_value => Err(report!("error unknown normalized arch value")
                .attach(format!("value={value:?} normalized_value={normalized_value:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("x86_64", Arch::X86)]
    #[case("arm64", Arch::Arm)]
    #[case("X86_64", Arch::X86)]
    #[case("ARM64", Arch::Arm)]
    fn arch_from_str_when_valid_input_returns_expected_arch(#[case] input: &str, #[case] expected: Arch) {
        let result = Arch::from_str(input);
        assert2::assert!(let Ok(arch) = result);
        pretty_assertions::assert_eq!(arch, expected);
    }

    #[test]
    fn arch_from_str_when_unknown_input_returns_error_with_message() {
        let result = Arch::from_str("unknown");
        assert2::assert!(let Err(err) = result);
        assert!(err.to_string().contains("error unknown normalized arch value"));
    }

    #[rstest]
    #[case("darwin", Os::MacOs)]
    #[case("linux", Os::Linux)]
    #[case("DARWIN", Os::MacOs)]
    #[case("LINUX", Os::Linux)]
    fn os_from_str_when_valid_input_returns_expected_os(#[case] input: &str, #[case] expected: Os) {
        let result = Os::from_str(input);
        assert2::assert!(let Ok(os) = result);
        pretty_assertions::assert_eq!(os, expected);
    }

    #[test]
    fn os_from_str_when_unknown_input_returns_error_with_message() {
        let result = Os::from_str("unknown");
        assert2::assert!(let Err(err) = result);
        assert!(err.to_string().contains("error unknown normalized os value"));
    }
}
