use std::borrow::Cow;
use std::process::Command;

use chrono::DateTime;
use chrono::FixedOffset;
use color_eyre::eyre::eyre;
use color_eyre::owo_colors::OwoColorize;
use utils::cmd::CmdExt;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

/// Get all local and remote git refs sorted by latest to oldest modified.
///
/// Returns an error as soon as 1 single item cannot be converted to a [`GitRef`].
pub fn get_local_and_remote_refs() -> color_eyre::Result<Vec<GitRef>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "refs/heads/",
            "refs/remotes/",
            &format!("--format={}", GitRef::format()),
        ])
        .exec()?;

    let mut res = vec![];
    for line in std::str::from_utf8(&output.stdout)?.trim().split('\n') {
        res.push(<GitRef as std::str::FromStr>::from_str(line)?);
    }

    Ok(res)
}

/// Deduplicates local and remote git refs.
pub fn keep_local_and_untracked_refs(git_refs: &mut Vec<GitRef>) {
    let mut local_names = std::collections::HashSet::new();

    git_refs.retain(|x| {
        if x.remote.is_none() {
            local_names.insert(x.name.clone());
            true
        } else {
            !local_names.contains(&x.name)
        }
    });
}

#[derive(Clone, Debug)]
#[cfg_attr(test, derive(PartialEq))]
pub struct GitRef {
    name: String,
    remote: Option<String>,
    object_name: String,
    object_type: String,
    committer_email: String,
    committer_date_time: DateTime<FixedOffset>,
    subject: String,
}

impl GitRef {
    const SEPARATOR: &str = "|";

    pub fn format() -> String {
        [
            "%(refname)",
            "%(objectname:short)",
            "%(objecttype)",
            "%(committeremail)",
            "%(committerdate:rfc2822)",
            "%(subject)",
        ]
        .join(Self::SEPARATOR)
    }
}

impl SkimItem for GitRef {
    fn text(&self) -> Cow<'_, str> {
        Cow::from(self.name.clone())
    }

    fn preview(&self, _context: SkimPreviewContext) -> SkimItemPreview {
        SkimItemPreview::AnsiText(format!(
            "{}\n{} {} {}\n{} {}\n",
            self.subject.bold(),
            self.remote.as_deref().unwrap_or("local").red(),
            self.object_type.red(),
            self.object_name.red(),
            self.committer_date_time.green(),
            self.committer_email.blue().bold(),
        ))
    }
}

impl std::str::FromStr for GitRef {
    type Err = color_eyre::eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('|');

        let refname = parts
            .next()
            .ok_or_else(|| eyre!("missing refname in git for-each-ref output {s}"))?
            .to_string();

        let (name, remote) = if let Some(remote) = refname.strip_prefix("refs/remotes/") {
            remote
                .split_once('/')
                .map(|(refname, remote_name)| (remote_name, Some(refname)))
                .ok_or_else(|| eyre!("unexpected refs/remotes structure {refname}"))?
        } else {
            (refname.trim_start_matches("refs/heads/"), None)
        };

        Ok(GitRef {
            name: name.to_string(),
            remote: remote.map(str::to_string),
            object_name: parts
                .next()
                .ok_or_else(|| eyre!("missing objectname:short in git for-each-ref output {s}"))?
                .to_string(),
            object_type: parts
                .next()
                .ok_or_else(|| eyre!("missing objecttype in git for-each-ref output {s}"))?
                .to_string(),
            committer_email: parts
                .next()
                .ok_or_else(|| eyre!("missing committeremail in git for-each-ref output {s}"))?
                .to_string(),
            committer_date_time: parts
                .next()
                .map(DateTime::parse_from_rfc2822)
                .transpose()?
                .ok_or_else(|| eyre!("missing committerdate in git for-each-ref output {s}"))?,
            subject: parts
                .next()
                .ok_or_else(|| eyre!("missing subject in git for-each-ref output {s}"))?
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use chrono::TimeZone;

    use super::*;

    #[test]
    fn test_git_ref_from_str_works_as_expected() {
        let input = "refname|object_name|object_type|committer_email|Fri, 22 Aug 2025 13:55:07 +0200|subject";
        assert2::let_assert!(Ok(actual_git_ref) = GitRef::from_str(input));
        pretty_assertions::assert_eq!(
            GitRef {
                name: "refname".into(),
                remote: None,
                object_name: "object_name".into(),
                object_type: "object_type".into(),
                committer_email: "committer_email".into(),
                committer_date_time: FixedOffset::east_opt(2 * 3600)
                    .unwrap()
                    .with_ymd_and_hms(2025, 8, 22, 13, 55, 7)
                    .unwrap(),
                subject: "subject".into()
            },
            actual_git_ref
        );
    }
}
