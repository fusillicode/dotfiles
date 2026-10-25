---
name: branch-desc
description: Update an existing change request description to match published branch changes. Use for `$branch-desc`.
---

# Branch Description

Keep the current branch's existing change request description accurate, minimal, and limited to published changes.

1. Resolve repository, branch, project, host, and exact authenticated hosting-service username using the host's authenticated current-user operation; never infer identity. If its lookup form is unsupported, inspect the tool's documentation or read-only help and retry a supported read-only form; ask only when authentication, connectivity, or conflicting identity remains unresolved. Resolve exactly one open change request authored by that user from the current branch, with target/source SHAs, merge base, and description. Stop for ambiguity.
2. Inspect worktree and index; compare local `HEAD`, source remote ref, and change request source SHA. Stop if any change or commit is unpublished: describe only published change request state.
3. Read repository guidance, current description, complete merge-base-to-source diff, and commits. Treat the description as a hypothesis: keep accurate intent/context, correct or remove stale claims, and add material reviewer-relevant context without inventing business intent or scope. Ignore repository description templates and structures.
4. Read `../_shared/change-desc.md`, normalize and validate against it, and stop on violation. Do not write when the reconciled description is not meaningfully different.
5. Explicit `$branch-desc` authorizes one immediate description update only: never commit, push, or change title, draft state, target, assignees, labels, reviewers, or discussions.
6. Always use `# Why` then `# What`, each as a sequentially numbered list of lean, clear phrases. Explain material rationale in `# Why` and material changes in `# What`. Do not include ticket or issue references, verification claims, or verification results.
7. Update only the description of that exact change request, addressed by its repository and repository-scoped identifier. Submit only the description field; preserve all other fields. Pass dynamic CLI values as separate shell-escaped arguments, or use structured connector arguments. Re-read and report URL, source SHA, and changed sections.
