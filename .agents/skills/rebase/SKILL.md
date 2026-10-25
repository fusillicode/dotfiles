---
name: rebase
description: Preview or perform an explicitly requested rebase with conflict handling and exact-lease push support.
---

# Rebase

1. Resolve the repository, current branch, requested base, upstream remote, and full remote branch ref. Default the base to `master` only when the repository uses it.
2. Require a clean worktree and read repository guidance before mutation.
3. For a direct rebase, fetch the base and tracked remote branch. Record the current local SHA and remote SHA. For a preview, use the existing refs and record their OIDs without fetching.
4. Before a direct rebase, create and verify a unique local backup branch.
5. Preview mode performs no mutation, fetch, or ref update; report the prospective base and diff from existing refs. Direct mode performs only the named rebase.
6. Do not infer a global conflict strategy. Inspect each conflict and ask only when the correct resolution is ambiguous.
7. After conflict resolution, run focused checks covering the resolved files and directly affected dependents, then format per AGENTS.md. Formatter-only changes do not require rerunning focused checks.
8. Show the resulting commits and diff against the backup.
9. Push rewritten history only when explicitly requested. Fetch again and use an exact-object-ID `--force-with-lease` for the recorded full ref.
10. After a push, verify the remote equals local `HEAD`. Without a push, report local state only. On uncertainty, abort when possible and report the backup branch and exact state.

Report the base, backup, old and new SHAs, conflicts, push state, and recovery path.
