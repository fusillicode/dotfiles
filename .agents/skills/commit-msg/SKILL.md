---
name: commit-msg
description: Generate one copy-ready Git commit message from an explicit scope, staged changes, or one coherent set of local changes using local repository rules and conventions. Use for message-only requests or $commit-msg. Do not use when the same request asks to stage, commit, amend, push, publish, or address review feedback; let that mutation workflow choose and execute its own message.
---

# Commit Message

Generate one complete commit message without changing repository state. This workflow is read-only.

Resolve relative reference paths from this skill's source directory after resolving symlinks.

1. If the request also asks for a Git mutation, end this standalone workflow and let the mutation workflow use `../../shared/commit-msg.md`.
2. Resolve the repository, worktree, branch, and status.
3. Select the message scope in this order:
   1. Explicit user-supplied paths, range, or diff.
   2. The staged diff when nonempty.
   3. All tracked and untracked local changes only when they form one coherent semantic change.
   4. Ask one concise scope question when nothing changed, the scope is ambiguous, or the changes contain multiple semantic units.
4. Read the repository commit-message rules applicable to the selected scope, then read `../../shared/commit-msg.md`.
5. Inspect the selected changes and relevant local commits. Never fetch or query a hosting service; ask one concise question when a required base, ticket, or convention cannot be resolved locally.
6. Run local Git reads with `git --no-lazy-fetch --no-optional-locks -c core.fsmonitor=false`; add `--no-ext-diff --no-textconv` to diff commands. Use shell-safe dynamic arguments, put pathspecs after `--`, and construct ranges only from resolved OIDs. Do not run hooks or code checks.
7. Apply repository rules, the user's exact format where permitted, and the shared contract. Draft only from the selected changes; include a body or trailers only when required.
8. Validate the complete message against the applicable rules.
9. Before returning, confirm the selected repository state, applicable rules, changes, and local commits are unchanged. If any changed, stop and report stale input.
10. On success, return only the copy-ready message in a plain-text code block. On a blocker, return only the conflict or question needed to continue.
