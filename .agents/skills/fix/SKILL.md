---
name: fix
description: Repair findings from the immediately preceding or supplied audit. Use for `$fix`.
---

# Fix Audit Findings

Use a supplied audit result, else the immediately preceding audit. Ask for one if neither exists. A supplied audit must include `Scope`; ask if it does not. Do not use this skill for unrelated fixes.

`$fix` and `$fix all` select all findings. `$fix A1 A3` selects named findings. Prefer supplied blocks.

Treat A# as guidance. Use `Scope` to revalidate each selected finding immediately before editing. Mark a finding `stale` or `disproved` when its location, scenario, or risk no longer applies. Do not force a fix.

Ask and stop before a fix has materially different behavior, compatibility, security, data, or verification effects; the audit lacks a preserved behavior; or it crosses a declared public API, wire format, schema, storage, architecture, security, or money boundary. Otherwise choose the smallest behavior-preserving option while preserving unrelated non-format user changes.

## Workflow

Handle one finding at a time. Complete focused proof before the next finding.

1. Read guidance. Resolve the repository, branch, diff, target files, and A# finding. Read callers, tests, and contracts when the edit can affect them.
2. Revalidate `Issue`, `Repro`, `Constraint`, and `Proof`. Reproduce when practical. Otherwise record why and use code evidence and an available focused check.
3. Make the smallest fix that preserves the constraint and unrelated non-format user changes. Add or update a regression test when stable.
4. Run focused proof. Repair failures. If a finding remains failing or is blocked, report it and stop. Record `fixed`, `stale`, `disproved`, or `blocked`.
5. Revalidate remaining findings only when the edit can affect them.

Delegate long-running or multi-command proof execution to `check_runner` with exact commands and a writable log directory. Keep check selection, repairs, and gate acceptance in the parent. Reuse valid results while relevant inputs and environment remain unchanged.

After all selected findings, cover changed code and affected dependents with valid focused results; run only missing or invalidated checks. Then format per AGENTS.md and inspect the final diff. Claim a pass only when its exact focused command succeeds. Formatter-only changes do not require rerunning focused checks.

## Output

Report each selected finding in order:

```markdown
A1: fixed
```

For a finding not changed, use `stale`, `disproved`, or `blocked` and state the code evidence or exact blocker. Include `Repro` and `Proof` only for unresolved checks.
