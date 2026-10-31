# Repo work

Apply this guide before repo edits. Paths to personal guides are relative to the source directory defined in the personal `AGENTS.md`.

## Establish scope

1. Verify the repo, working directory, status, branch, and base.
2. Read applicable repo `AGENTS.md` files and relevant contributor, task-runner, and toolchain guidance. Do not preload unrelated documents.
3. If a dedicated worktree is requested, create a new sibling worktree for this task. Verify its branch and base before editing. Ask if an unspecified base could materially change the result.
4. Prefer a small behavior test before implementation when a stable test seam exists.

## Write readable code

Separate logical steps with blank lines. Avoid dense expressions.

Structure tests as Arrange, Act, Assert, with blank lines between setup, the operation, and assertions. Keep the operation distinct from assertions when practical. Add section comments only for complex tests. Do not invent setup or intermediate variables solely to fill each section.

## Verify affected behavior

After semantic edits, run focused tests, lint, builds, or purpose-built checks for changed code and directly affected dependents. Do not run repo-wide validation locally; CI owns broader validation. Replace overly broad commands with focused equivalents.

A check passes only when its exact command finishes successfully and satisfies the required gate. Warnings alone do not fail a successful command unless repo rules or the required gate reject them. Skipped required checks, timeouts, and environment failures do not pass. Never claim a replacement command proves the original command passed.

Wait for existing long-running checks; never start duplicate builds merely to obtain output. Reuse results only while relevant inputs and the environment remain unchanged. Later non-format edits invalidate affected results.

For Rust, use the repo's pinned toolchain and compatible commands. Never substitute a newer installed toolchain. Follow repo lint rules; use the personal lint baseline only when the repo specifies no lint rules. Apply the Rust guides at the triggers in the personal `AGENTS.md`.

## Format and inspect

After checks, format affected Rust packages; for other languages, use the repo-wide formatter. Formatter-only output needs no check rerun. Required formatter changes outside touched files are in scope; preserve unrelated non-format changes. Do not add manual cleanup to formatter output.

Before reporting completion, inspect the final diff and touched files for scope, behavior, stale comments, missing tests, and instruction compliance.

Keep routine reads quiet or bounded. Preserve complete errors, exit status, and diagnostic context; widen output only when needed.
