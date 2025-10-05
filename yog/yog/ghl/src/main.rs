#![feature(exit_status_error)]

use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use strum::IntoEnumIterator;
use ytil_github::pr::PullRequestLifecycle;
use ytil_github::pr::PullRequestSearch;
use ytil_github::pr::PullRequestStatus;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let Some(selected_op) = ytil_tui::minimal_select::<Op>(Op::iter().collect())? else {
        return Ok(());
    };

    match selected_op {
        Op::MergeDependabotPrs => merge_dependabod_prs()?,
    }

    Ok(())
}

#[derive(EnumIter)]
enum Op {
    MergeDependabotPrs,
}

impl core::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str_repr = match self {
            Self::MergeDependabotPrs => format!("{}", "Merge ALL dependabot PRs".bold()),
        };
        write!(f, "{str_repr}")
    }
}

fn merge_dependabod_prs() -> color_eyre::Result<()> {
    ytil_github::log_into_github()?;

    let prs = ytil_github::pr::get(
        "primait/es-be",
        &PullRequestSearch {
            author: Some("app/dependabot"),
            status: Some(PullRequestStatus::Success),
            lifecycle: Some(PullRequestLifecycle::Open),
        },
    )?;

    for pr in prs {
        let msg = match ytil_github::pr::merge(pr.number) {
            Ok(_) => format!("{} pr={} title={}", "Merged".green().bold(), pr.number, pr.title),
            Err(error) => format!(
                "{} pr={} title={} error={error:?}",
                "Error merging".red().bold(),
                pr.number,
                pr.title
            ),
        };
        println!("{msg}");
    }

    Ok(())
}
