//! Branch operations for Git repositories.
//!
//! Provides functions for retrieving default and current branch names, creating new branches,
//! switching branches, fetching branches from remotes, and listing all branches with metadata.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre::Context;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Cred;
use git2::RemoteCallbacks;
use git2::Repository;
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
pub fn get_default() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for getting default branch | path={}",
            repo_path.display()
        )
    })?;

    let default_remote_ref = crate::remote::get_default(&repo)?;

    let Some(target) = default_remote_ref.symbolic_target() else {
        bail!("error missing default branch");
    };

    Ok(target
        .split('/')
        .next_back()
        .ok_or_else(|| eyre!("error extracting default branch_name from target | target={target:?}"))?
        .to_string())
}

/// Get current branch name (fails if HEAD detached).
///
/// # Errors
/// - Repository discovery fails or HEAD is detached.
pub fn get_current() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for getting current branch | path={}",
            repo_path.display()
        )
    })?;

    if repo
        .head_detached()
        .wrap_err_with(|| eyre!("error checking if head is detached | path={}", repo_path.display()))?
    {
        bail!("error head is detached | path={}", repo_path.display())
    }

    repo.head()
        .wrap_err_with(|| eyre!("error getting head | path={}", repo_path.display()))?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| eyre!("error invalid branch shorthand UTF-8 | path={}", repo_path.display()))
}

/// Create a new local branch at current HEAD (no checkout).
///
/// # Errors
/// - Repository discovery, HEAD resolution, or branch creation fails.
pub fn create_from_default_branch(branch_name: &str, repo: Option<&Repository>) -> color_eyre::Result<()> {
    let repo = if let Some(repo) = repo {
        repo
    } else {
        let path = Path::new(".");
        &crate::repo::discover(path).wrap_err_with(|| {
            eyre!(
                "error getting repo for creating new branch | path={} branch={branch_name:?}",
                path.display()
            )
        })?
    };

    let commit = repo
        .head()
        .wrap_err_with(|| eyre!("error getting head | branch_name={branch_name:?}"))?
        .peel_to_commit()
        .wrap_err_with(|| eyre!("error peeling head to commit | branch_name={branch_name:?}"))?;

    repo.branch(branch_name, &commit, false)
        .wrap_err_with(|| eyre!("error creating branch | branch_name={branch_name:?}"))?;

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
pub fn push(branch_name: &str, repo: Option<&Repository>) -> color_eyre::Result<()> {
    let repo = if let Some(repo) = repo {
        repo
    } else {
        let path = Path::new(".");
        &crate::repo::discover(path).wrap_err_with(|| {
            eyre!(
                "error getting repo for pushing new branch | path={} branch={branch_name:?}",
                path.display()
            )
        })?
    };

    let default_remote = crate::remote::get_default(repo)?;

    let default_remote_name = default_remote
        .name()
        .ok_or_else(|| eyre!("error missing name of default remote"))?
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
    remote.push(&[&branch_refspec], Some(&mut push_opts)).wrap_err_with(|| {
        eyre!("error pushing branch to remote | branch_refspec={branch_refspec:?} default_remote_name={default_remote_name:?}")
    })?;

    Ok(())
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
/// - The repository cannot be discovered.
/// - The 'origin' remote cannot be found.
/// - Performing `git fetch` for all branches fails.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_all() -> color_eyre::Result<Vec<Branch>> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path)
        .wrap_err_with(|| eyre!("error getting repo for getting branches | path={}", repo_path.display()))?;

    fetch(&[]).wrap_err_with(|| eyre!("error fetching branches"))?;

    let mut out = vec![];
    for branch_res in repo
        .branches(None)
        .wrap_err_with(|| eyre!("error enumerating branches"))?
    {
        let branch = branch_res.wrap_err_with(|| eyre!("error getting branch result"))?;
        out.push(Branch::try_from(branch).wrap_err_with(|| eyre!("error creating branch from result"))?);
    }

    out.sort_by(|a, b| b.committer_date_time().cmp(a.committer_date_time()));

    Ok(out)
}

/// Retrieves all branches without redundant remote duplicates.
///
/// # Errors
/// - The repository cannot be discovered.
/// - The 'origin' remote cannot be found.
/// - Performing `git fetch` for all branches fails.
/// - Enumerating branches fails.
/// - A branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
pub fn get_all_no_redundant() -> color_eyre::Result<Vec<Branch>> {
    let mut branches = get_all()?;
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
pub fn fetch(branches: &[&str]) -> color_eyre::Result<()> {
    let repo_path = Path::new(".");
    let repo = crate::repo::discover(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for fetching branches | path={} branches={branches:?}",
            repo_path.display()
        )
    })?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
    });

    let mut fetch_opts = git2::FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    repo.find_remote("origin")
        .wrap_err_with(|| eyre!("error finding origin remote"))?
        .fetch(branches, Some(&mut fetch_opts), None)
        .wrap_err_with(|| eyre!("error fetching branches={branches:?}"))?;

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
    let mut local_names = HashSet::with_capacity(branches.len());
    for branch in branches.iter() {
        if let Branch::Local { name, .. } = branch {
            local_names.insert(name.clone());
        }
    }

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
        /// The date and time when the last commit was made.
        committer_date_time: DateTime<Utc>,
    },
    /// Remote tracking branch (under `refs/remotes/`).
    Remote {
        /// The name of the branch (without refs/heads/ or refs/remotes/ prefix).
        name: String,
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
/// Extracts the branch name and last committer date from the raw branch.
///
/// # Errors
/// - Branch name is not valid UTF-8.
/// - Resolving the branch tip commit fails.
/// - Converting the committer timestamp into a [`DateTime`] fails.
impl<'a> TryFrom<(git2::Branch<'a>, git2::BranchType)> for Branch {
    type Error = color_eyre::eyre::Error;

    fn try_from((raw_branch, branch_type): (git2::Branch<'a>, git2::BranchType)) -> Result<Self, Self::Error> {
        let branch_name = raw_branch
            .name()?
            .ok_or_else(|| eyre!("error invalid branch name UTF-8 | branch_name={:?}", raw_branch.name()))?;
        let commit_time = raw_branch.get().peel_to_commit()?.committer().when();
        let committer_date_time = DateTime::from_timestamp(commit_time.seconds(), 0)
            .ok_or_else(|| eyre!("error invalid commit timestamp | seconds={}", commit_time.seconds()))?;

        Ok(match branch_type {
            git2::BranchType::Local => Self::Local {
                name: branch_name.to_string(),
                committer_date_time,
            },
            git2::BranchType::Remote => Self::Remote {
                name: branch_name.to_string(),
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

        assert2::let_assert!(Ok(result) = Branch::try_from((branch, git2::BranchType::Local)));

        pretty_assertions::assert_eq!(
            result,
            Branch::Local {
                name: "test-branch".to_string(),
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
            committer_date_time: DateTime::from_timestamp(123_456, 0).unwrap(),
        },
        DateTime::from_timestamp(123_456, 0).unwrap()
    )]
    #[case::remote_variant(
        Branch::Remote {
            name: "origin/test".to_string(),
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
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    fn remote(name: &str) -> Branch {
        Branch::Remote {
            name: name.into(),
            committer_date_time: DateTime::from_timestamp(0, 0).unwrap(),
        }
    }
}
