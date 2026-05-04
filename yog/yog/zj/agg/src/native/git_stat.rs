pub fn run(cwd: &str) -> GitStat {
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return GitStat::default();
    };

    let (insertions, deletions) = repo
        .diff_index_to_workdir(None, None)
        .and_then(|d| d.stats())
        .map_or((0, 0), |s| (s.insertions(), s.deletions()));

    let new_files = repo
        .statuses(Some(
            git2::StatusOptions::new()
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .exclude_submodules(true)
                .include_ignored(false),
        ))
        .map_or(0, |st| {
            st.iter().filter(|s| s.status().contains(git2::Status::WT_NEW)).count()
        });

    let is_worktree = repo.is_worktree();

    GitStat {
        insertions,
        deletions,
        new_files,
        is_worktree,
    }
}

#[derive(Default)]
pub struct GitStat {
    insertions: usize,
    deletions: usize,
    new_files: usize,
    is_worktree: bool,
}

impl std::fmt::Display for GitStat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.insertions,
            self.deletions,
            self.new_files,
            u8::from(self.is_worktree),
        )
    }
}
