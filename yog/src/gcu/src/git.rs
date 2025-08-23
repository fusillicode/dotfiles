use std::borrow::Cow;
use std::process::Command;

use chrono::DateTime;
use chrono::FixedOffset;
use color_eyre::owo_colors::OwoColorize;
use serde::Deserialize;
use serde::Deserializer;
use utils::cmd::CmdExt;
use utils::sk::SkimItem;
use utils::sk::SkimItemPreview;
use utils::sk::SkimPreviewContext;

/// Get all local and remote [GitRef]'s sorted by latest to oldest modified.
///
/// Returns an error as soon as 1 single item cannot be converted to a [GitRef].
pub fn get_local_and_remote_refs() -> color_eyre::Result<Vec<GitRef>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "refs/heads/",
            "refs/remotes/",
            &format!("--format={}", GitRefJson::to_format()),
        ])
        .exec()?;

    let mut res = vec![];
    for line in std::str::from_utf8(&output.stdout)?.trim().split('\n') {
        res.push(serde_json::from_str(line)?);
    }

    Ok(res)
}

/// Deduplicates local and remote [GitRef]s.
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

#[derive(Deserialize)]
struct GitRefJson {
    ref_name: String,
    object_name: String,
    object_type: String,
    committer_email: String,
    #[serde(deserialize_with = "deserialize_from_rfc2822")]
    committer_date_time: DateTime<FixedOffset>,
    subject: String,
}

fn deserialize_from_rfc2822<'de, D>(deserializer: D) -> Result<DateTime<FixedOffset>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    DateTime::parse_from_str(&s, "%a, %d %b %Y %H:%M:%S %z").map_err(serde::de::Error::custom)
}

impl GitRefJson {
    pub fn to_format() -> serde_json::Value {
        serde_json::json!({
            "ref_name": "%(refname)",
            "object_name": "%(objectname:short)",
            "object_type": "%(objecttype)",
            "committer_email": "%(committeremail)",
            "committer_date_time": "%(committerdate:rfc2822)",
            "subject": "%(subject:sanitize)",
        })
    }
}

impl<'de> Deserialize<'de> for GitRef {
    fn deserialize<D>(deserializer: D) -> Result<GitRef, D::Error>
    where
        D: Deserializer<'de>,
    {
        let git_ref_json = GitRefJson::deserialize(deserializer)?;

        let ref_name = git_ref_json.ref_name;
        let (name, remote) = if let Some(remote) = ref_name.strip_prefix("refs/remotes/") {
            remote
                .split_once('/')
                .map(|(refname, remote_name)| (remote_name, Some(refname)))
                .ok_or_else(|| serde::de::Error::custom(format!("unexpected refs/remotes structure {ref_name}")))?
        } else {
            (ref_name.trim_start_matches("refs/heads/"), None)
        };

        Ok(GitRef {
            name: name.to_string(),
            remote: remote.map(ToOwned::to_owned),
            object_name: git_ref_json.object_name,
            object_type: git_ref_json.object_type,
            committer_email: git_ref_json.committer_email,
            committer_date_time: git_ref_json.committer_date_time,
            // To kinda work around %(subject:sanitize) that is required to avoid broken JSONs...
            subject: git_ref_json.subject.replace("-", " "),
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn test_git_ref_deserialization_succeeds() {
        let input = serde_json::json!({
            "ref_name": "refname",
            "object_name": "object_name",
            "object_type": "object_type",
            "committer_email": "committer_email",
            "committer_date_time": "Fri, 22 Aug 2025 13:55:07 +0200",
            "subject": "subject-foo",
        })
        .to_string();

        let res = serde_json::from_str(&input);

        assert2::let_assert!(Ok(actual_git_ref) = res);
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
                subject: "subject foo".into()
            },
            actual_git_ref
        );
    }
}
