# Delegation

Apply this guide before delegating. Use the role routing and ownership rules in the personal `AGENTS.md`.

## Bound the work

Use one `code_reader` for a bounded question spanning repos. Split substantial independent questions into separate tasks. Do not split solely because files belong to different repos, or spawn one worker per command.

Select the named role explicitly; a generic task name does not load its configuration. If the client cannot apply the role and its tool constraints, keep the work in the parent.

## Supply a complete brief

Give each worker the exact scope, identifiers, baseline, constraints, completion criteria, and required evidence.

| Work | Additional required context |
| --- | --- |
| Repo exploration | Absolute repo roots, bounded question, relevant baseline, source evidence required |
| Focused verification | Exact commands, workdir, relevant baseline, writable log directory |
| Review collection | Collection mode; distinguish `note_id` from `discussion_id` |
| Telemetry | Environment, service, UTC bounds, preferred backend when known |

Require status (`complete`, `partial`, or `blocked`), baseline, result, evidence, and limits. Exact bodies and diagnostics override brevity. The parent combines results and assigns follow-up work.

## Preserve permission boundaries

Read-only role instructions constrain behavior, not filesystem permissions. Parent-session permissions and live runtime overrides determine effective access. When enforced read-only access is required, use existing parent-session permission controls. Do not treat a role declaration as proof of isolation or change permissions without user authority.

## Manage context and evidence

Prefer fresh worker context; use `fork_turns="none"` where supported. Reuse workers while the objective, repo, and relevant baseline remain stable. Start fresh when these change or accumulated context is no longer relevant.

Do not repeat completed collection or checks without a concrete evidence gap. Reuse check results only while relevant inputs and the environment remain unchanged.
