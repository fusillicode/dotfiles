//! Branch operations for Git repositories.
//!
//! Provides functions for retrieving default and current branch names, creating new branches,
//! switching branches, fetching branches from remotes, and listing all branches with metadata.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use git2::Cred;
use git2::RemoteCallbacks;
use git2::Repository;
use rootcause::bail;
use rootcause::prelude::ResultExt;
use rootcause::report;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

/// Retrieves the default branch name from the Git repository.
///
/// Iterates over all configured remotes and returns the branch name pointed to by the first valid
/// `refs/remotes/{remote}/HEAD` reference.
///
/// # Errors
/// - If the repository cannot be opened.
/// - If no remote has a valid `HEAD` reference.
/// - If the branch name cannot be extracted from the reference target.
pub fn get_default() -> rootcause::Result<String> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path)
        .context("error getting repo for getting default branch")
        .attach_with(|| format!("path={}", repo_path.display()))?;

    let default_remote_ref = crate::remote::get_default(&repo)?;

    let Some(target) = default_remote_ref.symbolic_target() else {
        bail!("error missing default branch");
    };

    Ok(target
        .split('/')
        .next_back()
        .ok_or_else(|| report!("error extracting default branch_name from target"))
        .attach_with(|| format!("target={target:?}"))?
        .to_string())
}

/// Get current branch name (fails if HEAD detached).
///
/// # Errors
/// - Repository discovery fails or HEAD is detached.
pub fn get_current() -> rootcause::Result<String> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path)
        .context("error getting repo for getting current branch")
        .attach_with(|| format!("path={}", repo_path.display()))?;

    if repo
        .head_detached()
        .context("error checking if head is detached")
        .attach_with(|| format!("path={}", repo_path.display()))?
    {
        Err(report!("error head is detached")).attach_with(|| format!("path={}", repo_path.display()))?;
    }

    repo.head()
        .context("error getting head")
        .attach_with(|| format!("path={}", repo_path.display()))?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| report!("error invalid branch shorthand UTF-8"))
        .attach_with(|| format!("path={}", repo_path.display()))
}

/// Create a new local branch at current HEAD (no checkout).
///
/// # Errors
/// - Repository discovery, HEAD resolution, or branch creation fails.
pub fn create_from_default_branch(branch_name: &str, repo: Option<&Repository>) -> rootcause::Result<()> {
    let repo = if let Some(repo) = repo {
        repo
    } else {
        let path = Path::new(".");
        &crate::repo::discover(path)
            .context("error getting repo for creating new branch")
            .attach_with(|| format!("path={} branch={branch_name:?}", path.display()))?
    };

    let commit = repo
        .head()
        .context("error getting head")
        .attach_with(|| format!("branch_name={branch_name:?}"))?
        .peel_to_commit()
        .context("error peeling head to commit")
        .attach_with(|| format!("branch_name={branch_name:?}"))?;

    repo.branch(branch_name, &commit, false)
        .context("error creating branch")
        .attach_with(|| format!("branch_name={branch_name:?}"))?;

    Ok(())
}

/// Pushes a branch to the default remote.
///
/// Uses the default remote (determined by the first valid `refs/remotes/{remote}/HEAD` reference)
/// to push the specified branch.
///
/// # Errors
/// - Repository discovery fails.
/// - No default remote can be determined.
/// - The default remote cannot be found.
/// - Pushing the branch fails.
pub fn push(branch_name: &str, repo: Option<&Repository>) -> rootcause::Result<()> {
    let repo = if let Some(repo) = repo {
        repo
    } else {
        let path = Path::new(".");
        &crate::repo::discover(path)
            .context("error getting repo for pushing new branch")
            .attach_with(|| format!("path={} branch={branch_name:?}", path.display()))?
    };

    let default_remote = crate::remote::get_default(repo)?;

    let default_remote_name = default_remote
        .name()
        .ok_or_else(|| report!("error missing name of default remote"))?
        .trim_start_matches("refs/remotes/")
        .trim_end_matches("/HEAD");

    let mut remote = repo.find_remote(default_remote_name)?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
    });

    let mut push_opts = git2::PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let branch_refspec = format!("refs/heads/{branch_name}");
    remote
        .push(&[&branch_refspec], Some(&mut push_opts))
        .context("error pushing branch to remote")
        .attach_with(|| format!("branch_refspec={branch_refspec:?} default_remote_name={default_remote_name:?}"))?;

    Ok(())
}

/// Returns the name of the previously checked-out branch (`@{-1}`), if any.
///
/// Walks the HEAD reflog looking for the most recent checkout/switch entry and
/// extracts the source branch name from the message.
///
/// Returns [`None`] when there is no recorded previous branch (e.g. fresh clone)
/// or the reflog cannot be read.
pub fn get_previous(repo: &Repository) -> Option<String> {
    let reflog = repo.reflog("HEAD").ok()?;
    reflog.iter().find_map(|entry| {
        let msg = entry.message()?;
        let rest = msg
            .strip_prefix("checkout: moving from ")
            .or_else(|| msg.strip_prefix("switch: moving from "))?;
        Some(rest.rsplit_once(" to ")?.0.to_string())
    })
}

/// Checkout a branch or detach HEAD.
///
/// # Errors
/// - `git switch` command fails.
pub fn switch(branch_name: &str) -> Result<(), Box<CmdError>> {
    Command::new("git")
        .args(["switch", branch_name, "--guess"])
        .exec()
        .map_err(Box::new)?;
    Ok(())
}

/// Fetches all branches from the 'origin' remote and returns all local and remote [`Branch`]es
/// sorted by last committer date (most recent first).
///
/// # Errors
/// - The 'origin' remote cannot be found.
/// - Performing `git fetch` for all branches fails.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_all(repo: &Repository) -> rootcause::Result<Vec<Branch>> {
    fetch_with_repo(repo, &[]).context("error fetching branches")?;

    let mut out = vec![];
    for branch_res in repo.branches(None).context("error enumerating branches")? {
        let branch = branch_res.context("error getting branch result")?;
        out.push(Branch::try_from(branch).context("error creating branch from result")?);
    }

    out.sort_unstable_by(|a, b| b.committer_date_time().cmp(a.committer_date_time()));

    Ok(out)
}

/// Retrieves all branches without redundant remote duplicates.
///
/// # Errors
/// - The 'origin' remote cannot be found.
/// - Performing `git fetch` for all branches fails.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_all_no_redundant(repo: &Repository) -> rootcause::Result<Vec<Branch>> {
    let mut branches = get_all(repo)?;
    remove_redundant_remotes(&mut branches);
    Ok(branches)
}

/// Fetches the specified branch names from the `origin` remote.
///
/// Used before switching to a branch that may only exist remotely
/// (e.g. derived from a GitHub PR URL).
///
/// # Errors
/// - The repository cannot be discovered.
/// - The `origin` remote cannot be found.
/// - Performing `git fetch` for the requested branches fails.
pub fn fetch(branches: &[&str]) -> rootcause::Result<()> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path)
        .context("error getting repo for fetching branches")
        .attach_with(|| format!("path={} branches={branches:?}", repo_path.display()))?;
    fetch_with_repo(&repo, branches)
}

/// Fetches branches using a pre-discovered repository, avoiding redundant filesystem walks.
fn fetch_with_repo(repo: &Repository, branches: &[&str]) -> rootcause::Result<()> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    repo.find_remote("origin")
        .context("error finding origin remote")?
        .fetch(branches, Some(&mut fetch_opts), None)
        .context("error fetching branches")
        .attach_with(|| format!("branches={branches:?}"))?;

    Ok(())
}

/// Removes remote branches that have a corresponding local branch of the same
/// shortened name.
///
/// A remote branch is considered redundant if its name after the first `/`
/// (e.g. `origin/feature-x` -> `feature-x`) matches a local branch name.
///
/// After this function returns, each remaining [`Branch::Remote`] has no local
/// counterpart with the same short name.
pub fn remove_redundant_remotes(branches: &mut Vec<Branch>) {
    // Collect local branch names as owned `String`s. An owned `HashSet` is required because
    // `retain` takes `&mut self`, which conflicts with any `&str` borrows into the same vec.
    let local_names: HashSet<String> = branches
        .iter()
        .filter_map(|b| {
            if let Branch::Local { name, .. } = b {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();

    branches.retain(|b| match b {
        Branch::Local { .. } => true,
        Branch::Remote { name, .. } => {
            let short = name.split_once('/').map_or(name.as_str(), |(_, rest)| rest);
            !local_names.contains(short)
        }
    });
}

/// Local or remote branch with metadata about the last commit.
#[derive(Clone, Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum Branch {
    /// Local branch (under `refs/heads/`).
    Local {
        /// The name of the branch (without refs/heads/ or refs/remotes/ prefix).
        name: String,
        /// The email address of the last committer.
        committer_email: String,
        /// The date and time when the last commit was made.
        committer_date_time: DateTime<Utc>,
    },
    /// Remote tracking branch (under `refs/remotes/`).
    Remote {
        /// The name of the branch (without refs/heads/ or refs/remotes/ prefix).
        name: String,
        /// The email address of the last committer.
        committer_email: String,
        /// The date and time when the last commit was made.
        committer_date_time: DateTime<Utc>,
    },
}

impl Branch {
    /// Returns the branch name (no "refs/" prefix).
    pub fn name(&self) -> &str {
        match self {
            Self::Local { name, .. } | Self::Remote { name, .. } => name,
        }
    }

    /// Returns the branch name with the "origin/" prefix removed if present.
    pub fn name_no_origin(&self) -> &str {
        self.name().trim_start_matches("origin/")
    }

    /// Returns the email address of the last committer on this branch.
    pub fn committer_email(&self) -> &str {
        match self {
            Self::Local { committer_email, .. } | Self::Remote { committer_email, .. } => committer_email,
        }
    }

    /// Returns the timestamp of the last commit on this branch.
    pub const fn committer_date_time(&self) -> &DateTime<Utc> {
        match self {
            Self::Local {
                committer_date_time, ..
            }
            | Self::Remote {
                committer_date_time, ..
            } => committer_date_time,
        }
    }
}

/// Attempts to convert a libgit2 branch and its type into our [`Branch`] enum.
///
/// Extracts the branch name, last committer email and date from the raw branch.
///
/// # Errors
/// - Branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Committer email is not valid UTF-8.
/// - Converting the committer timestamp into a [`DateTime`] fails.
impl<'a> TryFrom<(git2::Branch<'a>, git2::BranchType)> for Branch {
    type Error = rootcause::Report;

    fn try_from((raw_branch, branch_type): (git2::Branch<'a>, git2::BranchType)) -> Result<Self, Self::Error> {
        let branch_name = raw_branch
            .name()?
            .ok_or_else(|| report!("error invalid branch name UTF-8"))
            .attach_with(|| format!("branch_name={:?}", raw_branch.name()))?;
        let committer = raw_branch.get().peel_to_commit()?.committer().to_owned();
        let committer_email = committer
            .email()
            .ok_or_else(|| report!("error invalid committer email UTF-8"))
            .attach_with(|| format!("branch_name={branch_name:?}"))?
            .to_string();
        let committer_date_time = DateTime::from_timestamp(committer.when().seconds(), 0)
            .ok_or_else(|| report!("error invalid commit timestamp"))
            .attach_with(|| format!("seconds={}", committer.when().seconds()))?;

        Ok(match branch_type {
            git2::BranchType::Local => Self::Local {
                name: branch_name.to_string(),
                committer_email,
                committer_date_time,
            },
            git2::BranchType::Remote => Self::Remote {
                name: branch_name.to_string(),
                committer_email,
                committer_date_time,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use git2::Time;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::remote_same_short_name(
        vec![local("feature-x"), remote("origin/feature-x")],
        vec![local("feature-x")]
    )]
    #[case::no_redundant(
        vec![local("feature-x"), remote("origin/feature-y")],
        vec![local("feature-x"), remote("origin/feature-y")]
    )]
    #[case::multiple_mixed(
        vec![
            local("feature-x"),
            remote("origin/feature-x"),
            remote("origin/feature-y"),
            local("main"),
            remote("upstream/main")
        ],
        vec![local("feature-x"), remote("origin/feature-y"), local("main")]
    )]
    #[case::different_remote_prefix(
        vec![local("feature-x"), remote("upstream/feature-x")],
        vec![local("feature-x")]
    )]
    fn remove_redundant_remotes_cases(#[case] mut input: Vec<Branch>, #[case] expected: Vec<Branch>) {
        remove_redundant_remotes(&mut input);
        assert_eq!(input, expected);
    }

    #[test]
    fn branch_try_from_converts_local_branch_successfully() {
        let (_temp_dir, repo) = crate::tests::init_test_repo(Some(Time::new(42, 3)));

        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        let branch = repo.branch("test-branch", &head_commit, false).unwrap();

        assert2::assert!(let Ok(result) = Branch::try_from((branch, git2::BranchType::Local)));

        pretty_assertions::assert_eq!(
            result,
            Branch::Local {
                name: "test-branch".to_string(),
                committer_email: "test@example.com".to_string(),
                committer_date_time: DateTime::from_timestamp(42, 0).unwrap(),
            }
        );
    }

    #[rstest]
    #[case::local_variant(local("main"), "main")]
    #[case::remote_variant(remote("origin/feature"), "origin/feature")]
    fn branch_name_when_variant_returns_name(#[case] branch: Branch, #[case] expected: &str) {
        pretty_assertions::assert_eq!(branch.name(), expected);
    }

    #[rstest]
    #[case::local_no_origin(local("main"), "main")]
    #[case::remote_origin_prefix(remote("origin/main"), "main")]
    #[case::remote_other_prefix(remote("upstream/feature"), "upstream/feature")]
    fn branch_name_no_origin_when_name_returns_trimmed(#[case] branch: Branch, #[case] expected: &str) {
        pretty_assertions::assert_eq!(branch.name_no_origin(), expected);
    }

    #[rstest]
    #[case::local_variant(
        Branch::Local {
            name: "test".to_string(),
            committer_email: "a@b.com".to_string(),
            committer_date_time: DateTime::from_timestamp(123_456, 0).unwrap(),
        },
        DateTime::from_timestamp(123_456, 0).unwrap()
    )]
    #[case::remote_variant(
        Branch::Remote {
            name: "origin/test".to_string(),
            committer_email: "a@b.com".to_string(),
            committer_date_time: DateTime::from_timestamp(654_321, 0).unwrap(),
        },
        DateTime::from_timestamp(654_321, 0).unwrap()
    )]
    fn branch_committer_date_time_when_variant_returns_date_time(
        #[case] branch: Branch,
        #[case] expected: DateTime<Utc>,
    ) {
        pretty_assertions::assert_eq!(branch.committer_date_time(), &expected);
    }

    fn local(name: &str) -> Branch {
        Branch::Local {
            name: name.into(),
            committer_email: String::new(),
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    fn remote(name: &str) -> Branch {
        Branch::Remote {
            name: name.into(),
            committer_email: String::new(),
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }
}
