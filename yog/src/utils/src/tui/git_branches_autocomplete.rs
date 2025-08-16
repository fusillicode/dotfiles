// use std::collections::HashSet;
use std::process::Command;
use std::str::FromStr;

use color_eyre::eyre::eyre;
use inquire::Autocomplete;
use inquire::CustomUserError;
use inquire::autocompletion::Replacement;

use crate::cmd::CmdExt;

#[derive(Clone)]
pub struct GitBranchesAutocomplete {
    entries: Vec<Entry>,
}

impl GitBranchesAutocomplete {
    pub fn new() -> color_eyre::Result<Self> {
        Ok(Self {
            entries: get_all_local_branches()?.into_iter().collect(),
        })
    }
}

impl Autocomplete for GitBranchesAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        Ok(self
            .entries
            .iter()
            .filter_map(|entry| {
                entry
                    .branch_name
                    .contains(input)
                    .then_some(entry.to_string())
            })
            .collect())
    }

    fn get_completion(
        &mut self,
        input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<Replacement, CustomUserError> {
        if let Some(suggestion) = highlighted_suggestion {
            return Ok(Replacement::Some(suggestion));
        }
        Ok(self
            .get_suggestions(input)?
            .first()
            .and_then(|entry| {
                entry
                    .split("\n")
                    .next()
                    .map(|x| Replacement::Some(x.to_owned()))
            })
            .unwrap_or(Replacement::None))
    }
}

/// Get all local branches sorted by latest to oldest modified.
fn get_all_local_branches() -> color_eyre::Result<Vec<Entry>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "refs/heads/",
            "refs/remotes/",
            "--format=%(refname:short)|%(committeremail)|%(committerdate:iso8601)|%(subject)",
        ])
        .exec()?;

    Ok(std::str::from_utf8(&output.stdout)?
        .trim()
        .split('\n')
        .map(Entry::from_str)
        .filter_map(Result::ok)
        .collect())
}

#[derive(Clone)]
struct Entry {
    branch_name: String,
    committer_email: String,
    commit_iso8601_date: String,
    subject: String,
}

impl std::fmt::Display for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}\n{} {}\n{}",
            self.branch_name, self.commit_iso8601_date, self.committer_email, self.subject
        )
    }
}

impl FromStr for Entry {
    type Err = color_eyre::eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('|');

        Ok(Entry {
            branch_name: parts
                .next()
                .ok_or_else(|| eyre!("missing refname in parts {parts:#?}"))?
                .into(),
            committer_email: parts
                .next()
                .ok_or_else(|| eyre!("missing committeremail in parts {parts:#?}"))?
                .to_string(),
            commit_iso8601_date: parts
                .next()
                .ok_or_else(|| eyre!("missing committerdate in parts {parts:#?}"))?
                .to_string(),
            subject: parts
                .next()
                .ok_or_else(|| eyre!("missing subject in parts {parts:#?}"))?
                .to_string(),
        })
    }
}

// Removes all "origin" prefixes from branches, the "origin" branch and deduplicates the remotes
// and local branches.
// fn dedup_remotes(branches: &[Entry]) -> Vec<Entry> {
//     const DEFAULT_REMOTE: &str = "origin";
//     let mut seen = HashSet::new();
//     let mut out: Vec<_> = branches
//         .iter()
//         .map(|x| {
//             x.branch_name
//                 .trim_start_matches(&format!("{DEFAULT_REMOTE}/"))
//                 .to_string()
//         })
//         .filter(|x| x != DEFAULT_REMOTE)
//         .collect();
//     out.retain(|x| seen.insert(x.clone()));
//     out
// }

// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_dedup_remotes_works_as_expected() {
//         assert_eq!(
//             vec!["foo", "bar", "baz"],
//             dedup_remotes(&[
//                 "origin/foo".to_string(),
//                 "bar".to_string(),
//                 "origin".to_string(),
//                 "origin/foo".to_string(),
//                 "origin/bar".to_string(),
//                 "origin/baz".to_string()
//             ])
//         )
//     }
// }
