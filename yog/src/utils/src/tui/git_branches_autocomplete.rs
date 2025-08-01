use std::collections::HashSet;
use std::process::Command;

use inquire::autocompletion::Replacement;
use inquire::Autocomplete;
use inquire::CustomUserError;

use crate::cmd::CmdExt;

#[derive(Clone)]
pub struct GitBranchesAutocomplete {
    branches: Vec<String>,
}

impl GitBranchesAutocomplete {
    pub fn new() -> color_eyre::Result<Self> {
        Ok(Self {
            branches: dedup_remotes(&get_all_local_branches()?),
        })
    }
}

impl Autocomplete for GitBranchesAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        Ok(self
            .branches
            .iter()
            .filter_map(|branch| branch.contains(input).then_some(branch.clone()))
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
            .map(|branch| Replacement::Some(branch.clone()))
            .unwrap_or(Replacement::None))
    }
}

/// Get all local branches sorted by latest to oldest modified.
fn get_all_local_branches() -> color_eyre::Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "refs/heads/",
            "refs/remotes/",
            "--format=%(refname:short)",
        ])
        .exec()?;

    Ok(std::str::from_utf8(&output.stdout)?
        .trim()
        .split('\n')
        .map(str::to_string)
        .collect())
}

/// Removes all "origin" prefixes from branches, the "origin" branch and deduplicates the remotes
/// and local branches.
fn dedup_remotes(branches: &[String]) -> Vec<String> {
    const DEFAULT_REMOTE: &str = "origin";
    let mut seen = HashSet::new();
    let mut out: Vec<_> = branches
        .iter()
        .map(|x| {
            x.trim_start_matches(&format!("{DEFAULT_REMOTE}/"))
                .to_string()
        })
        .filter(|x| x != DEFAULT_REMOTE)
        .collect();
    out.retain(|x| seen.insert(x.clone()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_remotes_works_as_expected() {
        assert_eq!(
            vec!["foo", "bar", "baz"],
            dedup_remotes(&[
                "origin/foo".to_string(),
                "bar".to_string(),
                "origin".to_string(),
                "origin/foo".to_string(),
                "origin/bar".to_string(),
                "origin/baz".to_string()
            ])
        )
    }
}
