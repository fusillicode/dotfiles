use std::ops::Deref;

use color_eyre::owo_colors::OwoColorize;
use strum::EnumIter;
use ytil_git::GitStatusEntry;
use ytil_git::IndexState;
use ytil_git::WorktreeState;

/// Newtype wrapper adding colored [`core::fmt::Display`] for a [`ytil_git::GitStatusEntry`].
///
/// Renders two status columns (index + worktree) plus the path, dimming ignored entries
/// and prioritizing conflict markers.
///
/// # Examples
/// ```no_run
/// # fn show(e: &gch::RenderableGitStatusEntry) {
/// println!("{e}");
/// # }
/// ```
///
/// # Rationale
/// Needed to implement [`std::fmt::Display`] without modifying an external type (orphan rule).
///
/// # Performance
/// Only constructs small colored string fragments per render.
///
/// # Future Work
/// - Provide a structured render method (symbols + path) for alternative UIs.
pub struct RenderableGitStatusEntry(pub GitStatusEntry);

impl Deref for RenderableGitStatusEntry {
    type Target = GitStatusEntry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl core::fmt::Display for RenderableGitStatusEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Conflict overrides everything
        if self.conflicted {
            return write!(f, "{} {}", "CC".red().bold(), self.path.display().bold());
        }

        let index_symbol = self.index_state.as_ref().map_or_else(
            || " ".to_string(),
            |s| match s {
                IndexState::New => "A".green().bold().to_string(),
                IndexState::Modified => "M".yellow().bold().to_string(),
                IndexState::Deleted => "D".red().bold().to_string(),
                IndexState::Renamed => "R".cyan().bold().to_string(),
                IndexState::Typechange => "T".magenta().bold().to_string(),
            },
        );

        let worktree_symbol = self.worktree_state.as_ref().map_or_else(
            || " ".to_string(),
            |s| match s {
                WorktreeState::New => "A".green().bold().to_string(),
                WorktreeState::Modified => "M".yellow().bold().to_string(),
                WorktreeState::Deleted => "D".red().bold().to_string(),
                WorktreeState::Renamed => "R".cyan().bold().to_string(),
                WorktreeState::Typechange => "T".magenta().bold().to_string(),
                WorktreeState::Unreadable => "U".red().bold().to_string(),
            },
        );

        // Ignored marks as dimmed
        let (index_symbol, worktree_symbol) = if self.ignored {
            (index_symbol.dimmed().to_string(), worktree_symbol.dimmed().to_string())
        } else {
            (index_symbol, worktree_symbol)
        };

        write!(f, "{}{} {}", index_symbol, worktree_symbol, self.path.display())
    }
}

/// High-level Git working tree/index operations exposed by the UI.
#[derive(EnumIter)]
pub enum GitOperation {
    /// Add path contents to the index similar to `git add <path>`.
    Add,
    /// Discard changes in the worktree and/or reset the index for a path
    /// similar in spirit to `git restore` / `git checkout -- <path>`.
    Discard,
}

impl core::fmt::Display for GitOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str_repr = match self {
            Self::Discard => format!("{}", "Discard".red().bold()),
            Self::Add => "Add".green().bold().to_string(),
        };
        write!(f, "{str_repr}")
    }
}
