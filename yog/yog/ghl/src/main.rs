#![feature(exit_status_error)]

use std::ops::Deref;
use std::str::FromStr;

use color_eyre::owo_colors::OwoColorize;
use ytil_github::pr::PullRequest;
use ytil_github::pr::PullRequestMergeState;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ytil_github::log_into_github()?;

    let repo = ytil_github::get_current_repo()?;

    let args = ytil_system::get_args();
    let args: Vec<_> = args.iter().map(String::as_str).collect();

    let search_filter = args.first().copied();
    let merge_state = args.get(1).copied().map(PullRequestMergeState::from_str).transpose()?;

    let params = format!(
        "search_filter={search_filter:?}{}",
        merge_state.map(|ms| format!(" merge_state={ms:?}")).unwrap_or_default()
    );
    println!("\n{} {}", "Search PRs by".cyan().bold(), params.bold());

    let pull_requests = ytil_github::pr::get(&repo, search_filter, &|pr: &PullRequest| {
        if let Some(merge_state) = merge_state {
            return pr.merge_state == merge_state;
        }
        true
    })?;

    let renderable_prs = pull_requests.into_iter().map(RenderablePullRequest).collect();
    let Some(selected_prs) = ytil_tui::minimal_multi_select::<RenderablePullRequest>(renderable_prs)? else {
        return Ok(());
    };

    let selected_prs = selected_prs.iter().map(Deref::deref).collect::<Vec<_>>();
    for pr in selected_prs {
        let msg = match ytil_github::pr::merge(pr.number) {
            Ok(_) => format!("{} pr={} title={}", "Merged".green().bold(), pr.number, pr.title),
            Err(error) => format!(
                "{} pr={} title={} error={}",
                "Error merging".red().bold(),
                pr.number,
                pr.title,
                format!("{error:?}").red().bold()
            ),
        };
        println!("{msg}");
    }

    Ok(())
}

pub struct RenderablePullRequest(pub PullRequest);

impl Deref for RenderablePullRequest {
    type Target = PullRequest;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::fmt::Display for RenderablePullRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = match self.merge_state {
            PullRequestMergeState::Behind => "Behind".yellow().to_string(),
            PullRequestMergeState::Blocked => "Blocked".red().bold().to_string(),
            PullRequestMergeState::Clean => "Clean".green().to_string(),
            PullRequestMergeState::Dirty => "Dirty".red().bold().to_string(),
            PullRequestMergeState::Draft => "Draft".blue().bold().to_string(),
            PullRequestMergeState::HasHooks => "HasHooks".magenta().bold().to_string(),
            PullRequestMergeState::Unknown => "Unknown".bold().to_string(),
            PullRequestMergeState::Unmergeable => "Unmergeable".red().bold().to_string(),
        };
        write!(
            f,
            "{} {} {state} {}",
            self.number.bold(),
            self.author.login.blue().bold(),
            self.title.bold()
        )
    }
}
