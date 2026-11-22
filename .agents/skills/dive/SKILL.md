---
name: dive
description: Pressure-test an idea, problem, or proposed task in a one-question-at-a-time interview. Produce a self-contained Markdown brief or implementation plan with a persistent Q/A ledger. Review the interview's artifact adversarially on request.
---

# Dive

Challenge the premise, surface the strongest material objection, and separate verified facts from assumptions.

## Interview

1. Use plain chat. Ask one material question per turn and wait. Before sending it, save it as the sole `Next question` under `Open points` and append it to the ledger.
2. Capture the working directory at interview start. Use the user's artifact path if supplied. Otherwise, create `dive-hhmm-ddmmyyyy-<slug>.md` in the parent of that directory. Use one local creation timestamp, with a zero-padded 24-hour time and day, month, and four-digit year. If the name is taken, keep the timestamp and append `-2`, `-3`, and so on to the slug. Create new artifacts without overwriting an existing file. Resume only an explicitly identified artifact. If the destination is unwritable, ask only for a writable path and wait. This setup prompt is outside the ledger.
3. Create the artifact before the first interview question. Share the artifact path on creation. If no interview is needed, write the completed brief directly. During the interview and review, the artifact is the only write target.

### Check prerequisites before asking

Before asking a question, check the facts and prior decisions that determine whether it matters. Reuse verified evidence. Repeat a check only if its applicability, freshness, or coverage could change the decision. Ask only when those prerequisites are settled and the answer can change scope, behavior, risk, or the recommendation. Drop it if the evidence rules it out. If a prerequisite is unresolved, run the smallest useful read-only check. If it remains unresolved and the user can settle it, briefly name the gap and ask about the prerequisite instead. Otherwise, record the gap as a blocker in the brief.

Delegate substantial prerequisite checks to read-only subagents under the applicable delegation rules. This includes tracing multiple code paths or inspecting logs. Give each subagent a bounded question and request a short verdict, source references, and remaining gaps. The lead agent handles quick checks, uses the evidence to choose the next question, and updates the artifact. If delegation is unavailable, try a small targeted check and follow the unresolved-prerequisite rule if needed.

When a question is blocked or the user parks it, clear `Next question` and keep the topic under `Open points` with the reason. Return to a blocked question once it is answerable, or to a parked question when the user chooses to resume it. Remove open points that no longer apply.

## Artifact and ledger

Use the [artifact shape](assets/dive-shape.md) for the Markdown layout. Read it for artifact creation, conversion from a brief to an implementation plan, or a format check. Replace placeholders and omit inapplicable sections and comments.

Keep the brief above the inline ledger. Include a numbered list when decisions are settled, even for one decision. Give reasons and sources for the facts that drive each decision. Include rejected options when their rationale prevents repeated work. Omit rediscoverable detail and duplicate history. Redact secrets, credentials, raw customer data, and sensitive identifiers from the artifact. If redaction leaves a dependency, record how an authorized agent can resolve it.

While exploring, record open decisions, blockers, and useful read-only checks. Clear `Next question` once answered or inapplicable. Omit it when no question is awaiting an answer. A settled decision brief can end with its conclusion and evidence. A ready implementation plan gives dependency-ordered steps, observable acceptance checks, and specific stop conditions. Include exact commands when known and distinguish existing work from changes proposed by the plan.

Under `## QA ledger`, append every interview question and answer as written, in conversation order, subject to the artifact's redaction rules. Use a fenced text block with local `DD-MM HH:MM` timestamps and SCREAMING_SNAKE_CASE tags. Use `AQ:` for agent question, `AA:` for agent answer, `UQ:` for user question, and `UA:` for user answer. Add four spaces before each continuation line, preserving its indentation. Save exchanges before asking the next question or finalizing. Omit an empty ledger.

Answer questions about the brief or an option in chat and log the exchange. Update the brief when the user also requests edits or settles a decision. For a clarification, preserve the original question in the ledger and recheck its prerequisites.

Apply settled answers without reasking. Apply prerequisite answers first, then recheck which decisions still apply. Verify a conditional answer's condition before treating it as a decision. Keep any unresolved condition in `Open points` if it affects readiness.

Filter reads locally and patch only affected sections. Preserve untouched text byte for byte.

## Pickup in another session

Read the brief first. Resume from `Next question` or the relevant open point. Consult only relevant excerpts of `## QA ledger` when needed.

Before continuing, compare the brief's goal, target, and constraints with the current request. Recheck material facts that may have changed. Pause if you find a material mismatch.

Carry the shape file’s pickup rule into each implementation plan.

## Readiness

If a ledger-only answer differs from the saved readiness state, report both and explain the difference. Apply the rules below when updating or reviewing the brief.

For material factual conflicts, follow the prerequisite rules and verify with read-only checks. For conflicting proposals, check which version the user accepted. Recheck accepted decisions when verified facts undermine their rationale. If a material decision remains open, keep the artifact exploring and follow the interview rules to resolve it. Align the Dive artifact with verified facts and accepted decisions, and flag stale sources.

Set `State: ready` when the brief stands on its own and no unresolved issue would materially change it. Pending material decisions and verified blockers keep it `exploring`. Record dependencies and checks the plan already accounts for as implementation steps or verification.

Give one readiness verdict for the whole artifact, using the same state in chat. If exploring, name what blocks readiness. When reporting multiple blockers, dependencies, or verification checks in chat, use numbered lists with one actionable point per item. Report the artifact path when the interview ends. Implementation requires a separate user request.

## Requested adversarial review

Read [adversarial review instructions](references/adversarial-review.md) only when the user requests a review of this interview's artifact.
