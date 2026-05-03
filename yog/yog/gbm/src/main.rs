use std::fmt::Display;
use std::ops::Deref;
use std::path::Path;

use owo_colors::OwoColorize as _;
use rootcause::prelude::ResultExt as _;
use rootcause::report;
use ytil_git::branch::Branch;
use ytil_sys::cli::Args as _;

const ZSHRC_INSTALL_LINE: &str = r#"eval "$(gbm init zsh)""#;
const ZSH_WRAPPER: &str = r#"gbm() {
  if (( $# == 0 )); then
    local branch
    branch="$(command gbm --pick)" || return
    [[ -n "$branch" ]] || return
    print -z -- "gbm ${(q)branch}"
    return
  fi

  command gbm "$@"
}
"#;

/// Prepare or execute a current Git branch rename.
#[ytil_sys::main]
fn main() -> rootcause::Result<()> {
    let args = ytil_sys::cli::get();
    if args.has_help() {
        println!("{}", include_str!("../help.txt"));
        return Ok(());
    }

    let args: Vec<_> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        [] => Err(report!("gbm shell wrapper is not installed or loaded")
            .attach("run `gbm install` first, then restart zsh or source ~/.zshrc")),
        ["--pick"] => pick_git_branch(),
        ["install"] => install_zsh_wrapper(),
        ["init", "zsh"] => {
            print!("{ZSH_WRAPPER}");
            Ok(())
        }
        [branch_name] => rename_current_branch(branch_name),
        _ => rename_current_branch(&args.join("-")),
    }
}

fn pick_git_branch() -> rootcause::Result<()> {
    let Some(branch) = select_branch_with_current_first()? else {
        return Ok(());
    };

    println!("{}", branch.name_no_origin());
    Ok(())
}

fn install_zsh_wrapper() -> rootcause::Result<()> {
    let zshrc = std::env::var("HOME")
        .context("error missing HOME environment variable")
        .map(|home| Path::new(&home).join(".zshrc"))?;

    install_zsh_wrapper_at(&zshrc)?;
    println!("{} gbm in {}", "Patched".green().bold(), zshrc.display());

    Ok(())
}

fn install_zsh_wrapper_at(path: &Path) -> rootcause::Result<bool> {
    let content = std::fs::read_to_string(path)
        .context("error reading zshrc")
        .attach_with(|| format!("path={}", path.display()))?;

    if content.lines().any(|line| line.trim() == ZSHRC_INSTALL_LINE) {
        return Ok(false);
    }

    let mut updated = content;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(ZSHRC_INSTALL_LINE);
    updated.push('\n');

    std::fs::write(path, updated)
        .context("error installing zshrc")
        .attach_with(|| format!("path={}", path.display()))?;

    Ok(true)
}

fn select_branch_with_current_first() -> rootcause::Result<Option<Branch>> {
    let repo = ytil_git::repo::discover(Path::new(".")).context("error discovering repo for branch selection")?;
    let branches = prioritize_current_branch_first(
        ytil_git::branch::get_all_no_redundant(&repo)?,
        ytil_git::branch::get_current()?.as_str(),
        ytil_git::branch::get_previous(&repo).as_deref(),
        ytil_git::branch::get_user_email(&repo)?.as_deref(),
    );

    let Some(branch) = ytil_tui::minimal_select(branches.into_iter().map(RenderableBranch).collect())? else {
        return Ok(None);
    };

    Ok(Some(branch.0))
}

fn rename_current_branch(branch_name: &str) -> rootcause::Result<()> {
    ytil_git::branch::rename_current(branch_name, None)?;
    println!("{} {}", ">".magenta().bold(), branch_name.bold());
    Ok(())
}

fn prioritize_current_branch_first(
    branches: Vec<Branch>,
    current_branch: &str,
    previous_branch: Option<&str>,
    user_email: Option<&str>,
) -> Vec<Branch> {
    let branches = prioritize_recent_branches(branches, previous_branch, user_email);
    let mut current = None;
    let mut rest = Vec::with_capacity(branches.len());

    for branch in branches {
        if current.is_none() && branch.name_no_origin() == current_branch {
            current = Some(branch);
        } else {
            rest.push(branch);
        }
    }

    current.into_iter().chain(rest).collect()
}

fn prioritize_recent_branches(
    branches: Vec<Branch>,
    previous_branch: Option<&str>,
    user_email: Option<&str>,
) -> Vec<Branch> {
    const MINE_DESIRED_COUNT: usize = 5;

    let branches_len = branches.len();
    let mut previous = None;
    let mut mine = Vec::new();
    let mut rest = Vec::new();

    for branch in branches {
        if previous.is_none() && previous_branch.is_some_and(|prev| branch.name_no_origin() == prev) {
            previous = Some(branch);
        } else if mine.len() < MINE_DESIRED_COUNT && user_email.is_some_and(|email| branch.committer_email() == email) {
            mine.push(branch);
        } else {
            rest.push(branch);
        }
    }

    let mut prioritized = Vec::with_capacity(branches_len);
    prioritized.extend(previous);
    prioritized.extend(mine);
    prioritized.extend(rest);
    prioritized
}

struct RenderableBranch(pub Branch);

impl Deref for RenderableBranch {
    type Target = Branch;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for RenderableBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let styled_date_time = format!("({})", self.committer_date_time());
        let styled_email = format!("<{}>", self.committer_email());
        write!(
            f,
            "{} {} {}",
            self.name(),
            styled_date_time.green(),
            styled_email.blue().bold(),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use chrono::Utc;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        vec![branch("main", 30), branch("feature-a", 20), branch("feature-b", 10)],
        "feature-b",
        vec![branch("feature-b", 10), branch("main", 30), branch("feature-a", 20)]
    )]
    #[case(
        vec![remote_branch("origin/feature-a", 30), branch("main", 20)],
        "feature-a",
        vec![remote_branch("origin/feature-a", 30), branch("main", 20)]
    )]
    #[case(
        vec![branch("main", 30), branch("feature-a", 20)],
        "missing",
        vec![branch("main", 30), branch("feature-a", 20)]
    )]
    fn test_prioritize_current_branch_first_cases(
        #[case] branches: Vec<Branch>,
        #[case] current_branch: &str,
        #[case] expected: Vec<Branch>,
    ) {
        pretty_assertions::assert_eq!(
            prioritize_current_branch_first(branches, current_branch, None, None),
            expected
        );
    }

    #[test]
    fn test_prioritize_current_branch_first_preserves_gcu_recent_order_after_current() {
        let branches = vec![
            branch_with_email("other-1", "other@example.com", 100),
            branch_with_email("mine-1", "me@example.com", 99),
            branch_with_email("previous", "other@example.com", 98),
            branch_with_email("current", "me@example.com", 97),
            branch_with_email("mine-2", "me@example.com", 96),
        ];

        pretty_assertions::assert_eq!(
            prioritize_current_branch_first(branches, "current", Some("previous"), Some("me@example.com")),
            vec![
                branch_with_email("current", "me@example.com", 97),
                branch_with_email("previous", "other@example.com", 98),
                branch_with_email("mine-1", "me@example.com", 99),
                branch_with_email("mine-2", "me@example.com", 96),
                branch_with_email("other-1", "other@example.com", 100),
            ],
        );
    }

    #[test]
    fn test_install_zsh_wrapper_at_appends_line_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");
        std::fs::write(&zshrc, "source ~/.zshrc.local\n").unwrap();

        assert2::assert!(let Ok(true) = install_zsh_wrapper_at(&zshrc));
        let first = std::fs::read_to_string(&zshrc).unwrap();
        assert2::assert!(let Ok(false) = install_zsh_wrapper_at(&zshrc));
        let second = std::fs::read_to_string(&zshrc).unwrap();

        pretty_assertions::assert_eq!(first, second);
        pretty_assertions::assert_eq!(first, format!("source ~/.zshrc.local\n{ZSHRC_INSTALL_LINE}\n"));
        pretty_assertions::assert_eq!(first.matches(ZSHRC_INSTALL_LINE).count(), 1);
    }

    #[test]
    fn test_install_zsh_wrapper_at_fails_when_zshrc_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let zshrc = dir.path().join(".zshrc");

        assert2::assert!(let Err(err) = install_zsh_wrapper_at(&zshrc));

        assert!(err.to_string().contains("error reading zshrc"));
    }

    fn branch(name: &str, timestamp: i64) -> Branch {
        branch_with_email(name, "me@example.com", timestamp)
    }

    fn branch_with_email(name: &str, email: &str, timestamp: i64) -> Branch {
        Branch::Local {
            name: name.to_string(),
            committer_email: email.to_string(),
            committer_date_time: DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap(),
        }
    }

    fn remote_branch(name: &str, timestamp: i64) -> Branch {
        Branch::Remote {
            name: name.to_string(),
            committer_email: "me@example.com".to_string(),
            committer_date_time: DateTime::<Utc>::from_timestamp(timestamp, 0).unwrap(),
        }
    }
}
