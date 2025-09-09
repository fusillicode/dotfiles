use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use cmd::CmdExt;
use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize as _;

/// Returns a list of [`GitStatusEntry`] values parsed from
/// `git status --porcelain -s`.
///
/// # Errors
///
/// Returns an error if:
/// - Running the `git status` command fails.
/// - The command exits with a non‑zero status (see [`utils::cmd::CmdExt`]).
/// - The output cannot be decoded as valid UTF‑8.
/// - Any individual line cannot be parsed into a [`GitStatusEntry`].
pub fn get() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let stdout = Command::new("git").args(["status", "--porcelain", "-s"]).exec()?.stdout;
    let mut out = vec![];
    for entry in str::from_utf8(&stdout)?.lines() {
        out.push(GitStatusEntry::from_str(entry)?);
    }
    Ok(out)
}

/// Simplified representation of a porcelain `git status` line.
#[derive(Debug, Clone)]
pub enum GitStatusEntry {
    /// New / untracked file ("??").
    New(PathBuf),
    /// Added to the index ("A").
    Added(PathBuf),
    /// Modified file ("M").
    Modified(PathBuf),
    /// Renamed file ("R").
    Renamed(PathBuf),
    /// Deleted file ("D").
    Deleted(PathBuf),
}

impl FromStr for GitStatusEntry {
    type Err = color_eyre::Report;

    /// Parses a single porcelain status line into a [`GitStatusEntry`].
    ///
    /// # Errors
    ///
    /// Returns an error if the line does not match an expected `<code> <path>` pattern.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_ascii_whitespace().collect::<Vec<_>>().as_slice() {
            &["??", path] => Ok(Self::New(Path::new(path).into())),
            &["A", path] => Ok(Self::Added(Path::new(path).into())),
            &["M", path] => Ok(Self::Modified(Path::new(path).into())),
            &["R", path] => Ok(Self::Renamed(Path::new(path).into())),
            &["D", path] => Ok(Self::Deleted(Path::new(path).into())),
            &[] => bail!("cannot build GitStatusEntry from str {}", s),
            unexpected => bail!(
                "cannot build GitStatusEntry from str {}, unexpected parts {:#?}",
                s,
                unexpected
            ),
        }
    }
}

impl core::fmt::Display for GitStatusEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            Self::New(_) => "??".green().bold().to_string(),
            Self::Added(_) => "A".green().bold().to_string(),
            Self::Modified(_) => "M".yellow().bold().to_string(),
            Self::Renamed(_) => "R".blue().bold().to_string(),
            Self::Deleted(_) => "D".red().bold().to_string(),
        };
        write!(f, "{symbol} {}", self.file_path().display().bold())
    }
}

impl GitStatusEntry {
    /// The file path of the entry.
    pub fn file_path(&self) -> &Path {
        match self {
            Self::New(path) | Self::Added(path) | Self::Modified(path) | Self::Renamed(path) | Self::Deleted(path) => {
                path
            }
        }
    }
}
