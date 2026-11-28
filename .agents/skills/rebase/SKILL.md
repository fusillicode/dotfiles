---
name: rebase
description: Preview or perform explicitly requested single-branch or stacked rebases with conflict handling and exact-lease push support.
---

# Rebase

Read [Stacked branches](references/stacked-branches.md) before starting only when the request targets a stack, including previews.

1. Resolve the repository, current branch, requested base, upstream remote, and full remote branch ref. Default the base to `master` only when the repository uses it.
2. Require a clean worktree and read repository guidance before mutation.
3. For a preview, use existing refs and record their OIDs without fetching. For a direct rebase, fetch the base and record the original local tip. If a push is authorized, resolve the configured push URLs with `git remote get-url --push --all <remote>`. If there is more than one distinct URL, stop unless the user explicitly selected one. Use the selected endpoint for the original remote snapshot, the pre-push check, the push, and post-push verification.
   Bind the endpoint to an unused, command-scoped remote name by setting both `remote.<name>.url` and `remote.<name>.pushurl` to the selected URL with `git -c`. With those settings, use `git ls-remote --get-url -- <name>` to reject any further URL rewrite. The explicit push URL bypasses push-only rewriting. Use the same two settings for every operation on this endpoint without persisting them.
   Before creating backups or rewriting any branch, snapshot each ref authorized for push with `git ls-remote --exit-code --refs -- <name> <full-remote-ref>`. Verify the returned ref name. Exit 2 means the ref is absent. Other errors block the push workflow. If the ref exists, fetch it from the same endpoint, confirm its OID matches the snapshot, and require it to be an ancestor of the branch's original local tip. If it is not, report the remote-only commits and stop. If the ref is absent, record that state and proceed only with explicit authority to create it.
4. Before a direct rebase, create and verify a unique local backup branch.
5. Preview mode performs no mutation, fetch, or ref update. Report the prospective base and diff from existing refs. Immediately before starting each direct rebase, resolve the exact local branch ref again. Stop if it is missing or differs from the original recorded tip. Perform only the named rebase and pass `--no-update-refs` whether rebasing a single branch or a stack.
6. Do not infer a global conflict strategy. Inspect each conflict and ask only when the correct resolution is ambiguous.
7. After conflict resolution, run focused checks covering the resolved files and directly affected dependents, then format per AGENTS.md. Formatter-only changes do not require rerunning focused checks.
8. Show the resulting commits and diff against the backup.
9. Push only when explicitly requested. Recheck the endpoint binding and query the exact remote ref again. Stop if its state or OID differs from the original snapshot. Push the verified new OID with one explicit refspec:
   `git -c remote.<name>.url=<endpoint> -c remote.<name>.pushurl=<endpoint> push --no-follow-tags --recurse-submodules=no --force-with-lease=<full-remote-ref>:<expected-oid> -- <name> <verified-new-oid>:<full-remote-ref>`.
   Use the original remote OID for an existing ref. For authorized creation, leave the OID empty and keep the trailing colon so the lease requires the ref to stay absent.
   Pass each config setting, the lease, the remote name, and the refspec as separate shell-safe arguments.
10. After a push, verify the exact remote ref equals the verified new OID. Without a push, report local state only. On uncertainty, abort when possible and report the backup branch and exact state.

Report the base, backup, old and new SHAs, conflicts, push state, and recovery path.
