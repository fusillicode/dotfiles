---
name: check-comments
description: Deeply assess supplied review comments or all unresolved change request review discussions, validate concerns, and recommend durable solutions without changing code or remote discussions.
---

# Check Comments

Check comments is read-only. Validate the concern, find its root cause, and recommend a sound solution. Never edit, commit, push, reply, resolve, or otherwise mutate local or remote state.

## Method

1. Choose the input mode:
   - Supplied text: assess it as local input. Never guess its remote discussion.
   - Exact discussion IDs or URLs: fetch only those review discussions. Verify each current ID, body, author, anchor, and unresolved status. Stop if it is a note, resolved, or mismatched.
   - All, every, or broad feedback: resolve the exact change request and paginate unresolved review discussions only. Exclude top-level notes and resolved discussions.

   Delegate remote collection to the available role explicitly responsible for collecting review discussions with the selected input mode and exact identifiers. Keep concern assessment in the parent.
2. Resolve the repository, branch, and exact change request when remote context is needed. Record source and target SHAs, local `HEAD`, divergence, and dirty paths. Assess the change request source tree and diff, not unrelated local changes.
3. Read applicable repository guidance, changed code, callers, tests, configuration, contracts, and history. Trace each concern to a concrete location, reachable scenario, and evidence. Check relevant success, error, fallback, retry, cancellation, compatibility, security, data, performance, and architecture paths.
4. Classify each comment as `valid`, `partially valid`, `invalid`, `already fixed`, or `unclear`. Separate the comment's claim from verified facts. Do not reject a concern only because current tests pass.
5. For each valid or partially valid concern, identify the affected invariant or contract, root cause, edge cases, and materially different solutions. Recommend the solution that is correct, durable, and proportionate. Do not default to the smallest or easiest change. Explain relevant compatibility, API, wire, schema, storage, security, performance, operational, and migration tradeoffs.
6. Group duplicates and dependencies. Identify contradictory comments and separate correctness issues from preferences or product decisions. If evidence is insufficient, keep the item `unclear` and name the missing evidence or decision.
7. Stop before implementation or publication. Provide proposed changes and a focused verification sequence only for valid or partially valid concerns. Do not execute them.

`deep`, `super deep`, `thorough`, and `final` expand tracing and miss-hunts; they do not add output bookkeeping.

## Output

Start with the evidence baseline and comment scope. Use stable `C#` identifiers in input order:

```markdown
C1 <status> <file:line or discussion>

Finding: <claim; verified evidence and impact; assessment>
Fix: <recommended durable solution; only for valid or partially valid status>
Unknown: <uncertainty or blocker, or omit when none>
```

End with the overall recommendation. Include implementation and verification guidance only for recommended changes. Include risks and unresolved decisions only when material.
