---
name: review
description: Read-only change-request review with stable R# findings. Use only for `$review` or a top-level `review` command with a change-request URL.
---

# Review

Review is read-only. Preserve stable `R#` identifiers and concise output.

## Method

1. Resolve the exact URL, source SHA, target SHA, merge base, and current discussions. When an available discussion-collection role supports the resolved host, delegate collection to that role in all-discussions mode; otherwise collect in the parent. Reuse that snapshot in step 8. Keep review judgments in the parent.
2. Use an isolated worktree when the current checkout is dirty or on another task.
3. Read repository guidance, changed files, and directly affected artifacts. Start with relevant regions; read full files or callers, tests, configuration, and contracts when behavior or contract context requires it.
4. Map the changed behavior before looking for findings.
5. Trace changed behavior through affected callers, branches, variants, configuration, tests, and success, error, fallback, retry, cancellation, and compatibility paths.
6. Check the applicable risk areas: correctness, security, data loss, behavior regressions, concurrency, failure handling, performance, architecture, missing tests, and documentation. Prioritize executable behavior over docs or style while a changed behavior path remains untraced.
7. Treat each candidate as a hypothesis. Confirm or disprove it with a concrete location, reachable scenario, and source evidence.
8. Use the discussions fetched in step 1 to suppress duplicates.
9. Run focused read-only checks when they can validate a candidate or changed contract.
10. For a non-trivial review, or when a finding suggests another risk, perform a separate miss-hunt for a different failure class.
11. Before reporting, compare the current change request source SHA with the reviewed SHA. Stop if it changed; do not refresh unrelated metadata.

`deep`, `super deep`, `thorough`, and `final` expand caller, state, boundary, and failure-path tracing. They do not add output bookkeeping.

## Output

Use one block per finding:

```markdown
R1 P1 logic relative/path.rs:123

<one concrete review question>

Risk: <real consequence>
Issue: <wrong behavior and evidence>
Fix: <small fix and focused test>
```

Use priorities `P0` through `P3`. Use a concrete tag such as `logic`, `security`, `test`, `perf`, `arch`, or `docs`.

End with:

```markdown
URL: <reviewed URL>
Commit: <short SHA> | <subject>
Next: recheck
```

If no confirmed or concrete unresolved issue remains, output `No comments.` followed by the same footer with `Next: done`.
