use std::collections::HashSet;
use std::process::Command;

use color_eyre::eyre::bail;
use inquire::autocompletion::Replacement;
use inquire::Autocomplete;
use inquire::CustomUserError;

#[derive(Clone)]
pub struct GitBranchesAutocomplete {
    branches: Vec<String>,
}

impl GitBranchesAutocomplete {
    pub fn new() -> color_eyre::Result<Self> {
        Ok(Self {
            branches: dedup_remotes(&get_all_branches()?),
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

fn get_all_branches() -> color_eyre::Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-creatordate",
            "refs/heads/",
            "refs/remotes/",
            "--format=%(refname:short)",
        ])
        .output()?;
    if !output.status.success() {
        bail!("{}", std::str::from_utf8(&output.stderr)?.trim())
    }
    Ok(std::str::from_utf8(&output.stdout)?
        .trim()
        .split('\n')
        .map(str::to_string)
        .collect())
}

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
