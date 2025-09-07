use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use color_eyre::eyre::bail;
use color_eyre::owo_colors::OwoColorize as _;
use utils::cmd::CmdExt;
use utils::sk::SkimAnsiString;
use utils::sk::SkimDisplayContext;
use utils::sk::SkimItem;

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
pub fn get_git_status_entries() -> color_eyre::Result<Vec<GitStatusEntry>> {
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

impl SkimItem for GitStatusEntry {
    fn text(&self) -> std::borrow::Cow<'_, str> {
        Cow::from(match self {
            Self::New(path) => format!("?? {}", path.display()),
            Self::Added(path) => format!("A {}", path.display()),
            Self::Modified(path) => format!("M {}", path.display()),
            Self::Renamed(path) => format!("R {}", path.display()),
            Self::Deleted(path) => format!("D {}", path.display()),
        })
    }

    fn display<'a>(&'a self, _context: SkimDisplayContext<'a>) -> SkimAnsiString<'a> {
        // Colorize status code similarly to other tools in the workspace.
        // Keep path unmodified to maximize fuzzy‑matching signal.
        let (code, path) = match self {
            Self::New(path) => ("??".green().bold().to_string(), path),
            Self::Added(path) => ("A".green().bold().to_string(), path),
            Self::Modified(path) => ("M".yellow().bold().to_string(), path),
            Self::Renamed(path) => ("R".cyan().bold().to_string(), path),
            Self::Deleted(path) => ("D".red().bold().to_string(), path),
        };
        SkimAnsiString::from(format!("{code} {}", path.display()))
    }
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
