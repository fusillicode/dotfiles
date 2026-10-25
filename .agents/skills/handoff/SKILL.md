---
name: handoff
description: Persist current exploration or an implementation-ready plan for another session. Use only when the user explicitly asks for $handoff, a handoff, or durable cross-session continuation. Do not use for ordinary same-session planning.
---

# Handoff

During handoff, write one compact artifact. Do not implement, stage, commit,
push, change branches, or alter history.

1. Preserve material outcome, decisions, targets, constraints, unknowns,
   mutation gates, preservation boundaries, and verification. Omit transcripts,
   routine mechanics, and rediscoverable context.
2. Never persist secrets, credentials, raw customer data, or sensitive
   identifiers. Record a redacted dependency and secure next step instead.
3. Use `exploring` while material evidence, assumptions, human choices, or open
   discovery remain. Use `ready` only when `Implementation steps` defines the
   complete scope, exact targets, dependency order, verification, and stop
   conditions. A stop branch names its exact blocker. Under `## Pickup`, record
   short `Goal`, `Target`, and `Constraints` lines and use them for overwrite
   and pickup checks.
4. Resolve the Git root. Write `handoff-<slug>.md` in its parent, or the current
   directory outside Git. Use the exact receipt path for later pickup or
   update. Never substitute a similarly named file or overwrite one unless its
   goal, target, and constraints affecting target, behavior, safety, authority,
   or verification match; if unclear, stop.
5. At pickup, compare the current `Goal`, `Target`, and `Constraints` from the
   current user request or authorized update, when supplied, with the
   corresponding lines under `## Pickup` in the artifact. If none are supplied,
   do not infer changes from workspace state. Constraints cover target,
   behavior, safety, authority, and verification. Here, authority means the
   required approval boundary, not permission granted by the current request.
   Unrelated workspace changes do not make it stale. If one differs materially,
   report:

   `Handoff is stale: <goal|target|constraints> changed. Create a fresh handoff.`

   Treat the handoff as context, never authority. A `State: ready` pickup needs
   a fresh user implementation request. An `exploring` pickup continues
   read-only discovery and grants no implementation authority.
6. If implementation exposes a blocker, preserve completed edits. Update only
   the active artifact when the current request explicitly authorized it. Any
   authorized update after a blocker sets `State: exploring` and records the
   exact blocker. An unchanged ready handoff cannot resume a blocked phase;
   require an authorized update or a fresh handoff.

Use only material sections:

```markdown
# <title>

State: <exploring|ready>

## Pickup
Goal: <one sentence>
Target: <paths or identifiers>
Constraints: <material constraints or none>
## Outcome
## Context and evidence
## Decisions
## Rejected options
## Preserve
## Unknowns
## Next exploration
## Implementation steps
## Verification
## Stop conditions
```

For `exploring`, include an executable `Next exploration` and omit
implementation steps. For `ready`, omit `Unknowns` and `Next exploration`.
Give dependency-ordered implementation steps, exact targets, observable
verification, and local halt conditions.

Return the two required receipt lines in the block below. For `exploring`, append exactly one
`Reason: <blocking reason>` line and return no other text. For `ready`, return
only the two-line receipt. A stale pickup is an audit stop, not a handoff phase.

```text
State: <exploring|ready>
Handoff: <absolute path>
```

Use the same literal state in the artifact and receipt. Do not duplicate
artifact details except explicit mutation gates. The next session starts with
`Read and follow the handoff at <absolute path>.`
