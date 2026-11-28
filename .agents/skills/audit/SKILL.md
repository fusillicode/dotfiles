---
name: audit
description: Read-only audit of current local changes with stable A# findings. Use for `$audit` or a top-level `audit` request.
---

# Audit

Audit is read-only. Do not apply findings.

## Method

1. Resolve the target diff or named change. Read guidance and changed files. Read callers, tests, config, and contracts when the diff can affect them.
2. Map intended and preserved behavior. Trace changed paths and relevant success, error, fallback, retry, cancellation, and compatibility paths.
3. Select lenses from the diff's semantic impact. Use `logic` for behavior or policy, `rust` for Rust source or build changes, `test` for tests or stable test contracts, `perf` for hot paths or resource changes, `arch` for API, dependency, storage, or boundary changes, and `docs` for guidance or documentation. For `deep`, trace each applicable lens further. Keep small or overlapping traces in the parent. Delegate substantial independent evidence collection in bounded read-only packages, grouping lenses that need the same evidence. Give each worker only the relevant diff, files, and context.
4. Check only the applicable risk areas, including correctness, security, data loss, concurrency, performance, architecture, tests, and docs.
5. Confirm or disprove each candidate with a location, realistic scenario, and code evidence. Run focused read-only checks when useful.
6. For a non-trivial diff, or when a finding suggests another risk, make a short second pass for a different applicable failure class.

`deep`, `super deep`, `thorough`, and `final` expand tracing. They do not add output bookkeeping.

## Output

Start with `Scope: <audited local diff or named change>`.

Use one block per finding. Keep each field to one short sentence. Do not repeat facts.

```markdown
A1 P1 logic relative/path.rs:123

Risk: <real consequence>
Issue: <wrong behavior and evidence>
Repro: <inputs, state, and path to failure; or "Not practical: <reason>">
Constraint: <behavior or contract that the fix must preserve>
Fix: <small repair direction>
Proof: <focused test or check and expected result>
```

End findings with `Next: $fix A1` or `Next: $fix all`.

If no confirmed or concrete unresolved issue remains, output:

```markdown
All good.
Next: done
```
