//! Branch operations for Git repositories.
//!
//! Provides functions for retrieving default and current branch names, creating new branches,
//! switching branches, fetching branches from remotes, and listing all branches with metadata.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use chrono::DateTime;
use chrono::Utc;
use color_eyre::eyre::Context as _;
use color_eyre::eyre::bail;
use color_eyre::eyre::eyre;
use git2::Cred;
use git2::RemoteCallbacks;
use ytil_cmd::CmdError;
use ytil_cmd::CmdExt as _;

use crate::get_default_remote;
use crate::get_repo;

/// Retrieves the default branch name from the Git repository.
///
/// Iterates over all configured remotes and returns the branch name pointed to by the first valid
/// `refs/remotes/{remote}/HEAD` reference.
///
/// # Returns
/// The default branch name (e.g., "main" or "master").
///
/// # Errors
/// - If the repository cannot be opened.
/// - If no remote has a valid `HEAD` reference.
/// - If the branch name cannot be extracted from the reference target.
pub fn get_default() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)
        .wrap_err_with(|| eyre!("error getting default repo | repo_path={}", repo_path.display()))?;

    let default_remote_ref = get_default_remote(&repo)?;

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
/// # Returns
/// Branch short name (e.g. `main`).
///
/// # Errors
/// - Repository discovery fails.
/// - HEAD is detached.
/// - Branch shorthand not valid UTF-8.
///
/// # Future Work
/// - Provide enum distinguishing detached state instead of error.
pub fn get_current() -> color_eyre::Result<String> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)
        .wrap_err_with(|| eyre!("error getting current repo_path | path={}", repo_path.display()))?;

    if repo
        .head_detached()
        .wrap_err_with(|| eyre!("error checking if head is detached | repo_path={}", repo_path.display()))?
    {
        bail!("error head is detached | repo_path={}", repo_path.display())
    }

    repo.head()
        .wrap_err_with(|| eyre!("error getting head | repo_path={}", repo_path.display()))?
        .shorthand()
        .map(str::to_string)
        .ok_or_else(|| {
            eyre!(
                "error invalid branch shorthand UTF-8 | repo_path={}",
                repo_path.display()
            )
        })
}

/// Create a new local branch at current HEAD (no checkout).
///
/// Branch starts at the commit pointed to by `HEAD`; caller remains on the original branch.
///
/// # Arguments
/// - `branch_name` Name of branch to create (must not already exist).
///
/// # Returns
/// [`Result::Ok`] (()) if creation succeeds.
///
/// # Errors
/// - Repository discovery fails.
/// - Resolving `HEAD` to a commit fails.
/// - Branch already exists.
///
/// # Future Work
/// - Optionally force (move) existing branch with a flag.
/// - Support creating tracking configuration in one step.
pub fn create(branch_name: &str) -> color_eyre::Result<()> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for creating new branch | path={} branch={branch_name:?}",
            repo_path.display()
        )
    })?;

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
/// # Arguments
/// - `branch_name` Name of the branch to push (must exist locally).
///
/// # Returns
/// [`Result::Ok`] (()) if the push succeeds.
///
/// # Errors
/// - Repository discovery fails.
/// - No default remote can be determined.
/// - The default remote cannot be found.
/// - Pushing the branch fails.
pub fn push(branch_name: &str) -> color_eyre::Result<()> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for pushing new branch | path={} branch={branch_name:?}",
            repo_path.display()
        )
    })?;

    let default_remote = get_default_remote(&repo)?;

    let default_remote_name = default_remote
        .name()
        .ok_or_else(|| eyre!("error missing name of default remote"))?
        .trim_start_matches("refs/remotes/")
        .trim_end_matches("/HEAD");

    let mut remote = repo.find_remote(default_remote_name)?;

    let branch_refspec = format!("refs/heads/{branch_name}");
    remote.push(&[&branch_refspec], None).wrap_err_with(|| {
        eyre!("error pushing branch to remote | branch_refspec={branch_refspec:?} default_remote_name={default_remote_name:?}")
    })?;

    Ok(())
}

/// Checkout a branch or detach HEAD; supports previous branch shorthand and branch creation via guessing.
///
/// Defers to `git switch --guess` to leverage porcelain semantics, which can create a new branch
/// if the name is ambiguous and matches an existing remote branch.
///
/// # Arguments
/// - `branch_name` Branch name or revision (use `-` for prior branch).
///
/// # Errors
/// - Spawning or executing the `git switch` command fails.
///
/// # Future Work
/// - Expose progress callbacks for large checkouts.
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
pub fn get() -> color_eyre::Result<Vec<Branch>> {
    let repo_path = Path::new(".");
    let repo = get_repo(repo_path)
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
    let repo = get_repo(repo_path).wrap_err_with(|| {
        eyre!(
            "error getting repo for fetching branches | path={}",
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
    /// Returns the branch name (no `refs/` prefix).
    pub fn name(&self) -> &str {
        match self {
            Self::Local { name, .. } | Self::Remote { name, .. } => name,
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
