use agg::GitStat;

pub fn run(cwd: &str) -> GitStat {
    let path = cwd.into();
    let Ok(repo) = git2::Repository::discover(cwd) else {
        return GitStat {
            path,
            branch: None,
            ..Default::default()
        };
    };

    let branch = repo
        .head()
        .ok()
        .filter(git2::Reference::is_branch)
        .and_then(|head| head.shorthand().map(str::to_string));

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

    GitStat {
        path,
        branch,
        insertions,
        deletions,
        new_files,
        is_worktree: repo.is_worktree(),
    }
}
