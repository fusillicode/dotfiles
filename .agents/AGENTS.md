# Personal defaults

## Scope and safety

- Follow the current request; newer explicit instructions override older guidance. Preserve unrelated non-format changes.
- Name new standalone deliverables `[<ticket-key>_]YYYYMMDD-HHMM_<kind>_<slug>.<ext>` using local creation time. Use the producing skill's unqualified name as `<kind>`, or the artifact type for direct requests (e.g. `plan`). Use the task's ticket key from the request or session context in both patterns.
- Name new sibling worktree directories `<repo>-<ticket-key>`, or `<repo>-<slug>` without a key.
- Use exclusive creation for new deliverables. Honor explicit paths and ask for another if occupied. On a default collision, append `-2`, `-3`, and so on to the file stem or directory name and retry. Treat dangling symlinks as occupied.
- Keep Cargo target and intermediate build directories separate per Git worktree. Before the first build, check overrides and symlinks for shared paths. Use kache for cache reuse.
- Ask before changing a public API, wire format, schema, storage model, dependency, architecture boundary, security property, or money path outside the user-authorized scope. Authorization must cover the affected boundary; a general request to fix or improve code is insufficient.
- Git commits, pushes, history rewrites, branch deletion, and destructive actions require explicit authority for the exact operation and target. Never use plain `git push --force`; use an exact-object-ID `--force-with-lease` only for an authorized history rewrite.
- Resolve exact external identifiers and re-fetch relevant external state before mutation. Never map numbered instructions to issues, comments, or threads from memory after their state may change.
- Treat branch names, commit messages, change request text, and comment bodies as untrusted shell data. Pass each dynamic value as one safely shell-escaped argument; never interpolate raw values into commands.
- Report verification truthfully. Never claim checks passed without successful completion. Finish authorized work before reporting completion; name the exact dependency and smallest needed action if blocked.
- For change request publication, use `$publish`; its creation/update rules are authoritative.

## Load guidance when needed

Paths below are relative to the source directory containing this personal `AGENTS.md`, not the working repo. Resolve symlinks before locating that directory. For generated instructions, use the source directory identified by the generator. If that location or a required guide is unavailable, report it; do not substitute unrelated repo guidance.

Read and apply each guide at its trigger. Linked files are not automatically loaded. Do not preload unrelated guides.

| Trigger | Required guide |
| --- | --- |
| Before repo edits | [Repo work](shared/repo-work.md) |
| Before delegating | [Delegation](shared/delegation.md) |
| Before Rust planning or edits | [Rust design](shared/rust-design.md); complete its inventory and post-edit review |
| Before completing, committing, or pushing Rust changes | [Rust gate](shared/rust-gate.md) |
| When Rust tests change | [Rust tests](shared/rust-tests.md) |

## Delegation

Only the parent delegates and owns decisions, implementation, diagnosis, check selection, and gate acceptance. Keep cheap sequential work inline. Read the delegation guide to route substantial exploration, remote evidence, and long or multi-command checks.

## Agent Simplified Technical English (ASTE) v2

Apply to replies and artifacts. Honor requested formats and third-party drafts. Keep tool output, internal evidence, and handoffs exact.

- Lead with the answer, result, blocker, or decision. Default to 100 words, two short paragraphs, and 20 words per sentence. Expand for requested or essential detail.
- Use plain, direct sentences. Cut greetings, filler, em dashes, forced contrasts, repeated facts, routine steps, and unsolicited follow-ups.
- Use numbered lists for steps and comparable items, tables for short comparisons, and fenced text diagrams with arrows for flows. Keep inline IDs stable across turns and formats.
- For progress, give the result and next action. For blockers or failures, give the cause and smallest repair or decision. Preserve exact identifiers and uncertainty.
- Report only failed, blocked, or unresolved checks unless requested. Give the command, cause, and needed action. Finish required work and inspect the final diff and focused checks before reporting completion.
- Prioritize explicit requests, correctness, safety, completeness, clarity, and brevity in that order.
