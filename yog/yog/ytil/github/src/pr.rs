use std::process::Command;

use serde::Deserialize;
use ytil_cmd::CmdExt;

#[derive(Default)]
pub struct PullRequestSearch<'a> {
    pub author: Option<&'a str>,
    pub status: Option<PullRequestStatus>,
    pub lifecycle: Option<PullRequestLifecycle>,
    pub merge_state: Option<PullRequestMergeState>,
}

#[derive(strum::Display, Clone, Copy)]
pub enum PullRequestStatus {
    #[strum(to_string = "pending")]
    Pending,
    #[strum(to_string = "success")]
    Success,
    #[strum(to_string = "failure")]
    Failure,
}

#[derive(strum::Display, Clone, Copy)]
pub enum PullRequestLifecycle {
    #[strum(to_string = "open")]
    Open,
    #[strum(to_string = "closed")]
    Closed,
    #[strum(to_string = "merged")]
    Merged,
    #[strum(to_string = "unmerged")]
    Unmerged,
    #[strum(to_string = "draft")]
    Draft,
}

impl<'a> PullRequestSearch<'a> {
    pub fn to_gh_args(&self) -> Vec<String> {
        let mut search_args = vec![];
        if let Some(author) = self.author {
            search_args.push(format!("author:{author}"))
        }
        if let Some(status) = self.status {
            search_args.push(format!("status:{status}"))
        }
        if let Some(lifecycle) = self.lifecycle {
            search_args.push(format!("is:{lifecycle}"))
        }
        if search_args.is_empty() {
            return vec![];
        }
        vec!["--search".to_string(), search_args.join(" ")]
    }
}

#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: usize,
    pub title: String,
    pub merge_state: PullRequestMergeState,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PullRequestMergeState {
    Behind,
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    Unknown,
    Unmergeable,
}

pub fn get(repo: &str, search: &PullRequestSearch) -> color_eyre::Result<Vec<PullRequest>> {
    let mut args = vec!["pr", "list", "--repo", repo, "--json", "number,title,mergeStateStatus"];
    let search_args: Vec<_> = search.to_gh_args();
    let mut search_args = search_args.iter().map(String::as_str).collect();
    args.append(&mut search_args);

    let output = Command::new("gh").args(args).exec()?.stdout;
    if output.is_empty() {
        return Ok(vec![]);
    }

    let mut prs: Vec<PullRequest> = serde_json::from_slice(&output)?;
    if let Some(merge_state) = search.merge_state {
        prs.retain(|pr| pr.merge_state == merge_state);
    }

    Ok(prs)
}

pub fn merge(pr_number: usize) -> color_eyre::Result<()> {
    Command::new("gh")
        .args([
            "pr",
            "merge",
            "--admin",
            "--squash",
            "--delete-branch",
            &format!("{pr_number}"),
        ])
        .exec()?;
    Ok(())
}
