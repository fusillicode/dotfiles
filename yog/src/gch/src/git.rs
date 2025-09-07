use std::borrow::Cow;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use color_eyre::eyre::bail;
use utils::cmd::CmdExt;
use utils::sk::SkimItem;

pub fn get_git_status_entries() -> color_eyre::Result<Vec<GitStatusEntry>> {
    let stdout = Command::new("git").args(["status", "--porcelain", "-s"]).exec()?.stdout;
    let mut out = vec![];
    for entry in str::from_utf8(&stdout)?.lines() {
        out.push(GitStatusEntry::from_str(entry)?);
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub enum GitStatusEntry {
    New(PathBuf),
    Added(PathBuf),
    Modified(PathBuf),
    Renamed(PathBuf),
    Deleted(PathBuf),
}

impl GitStatusEntry {
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
}

impl FromStr for GitStatusEntry {
    type Err = color_eyre::Report;

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
