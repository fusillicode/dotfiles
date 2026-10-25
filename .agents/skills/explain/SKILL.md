---
name: explain
description: Build a verified mental model of code, systems, or product behavior. Use for `$explain` or explicit how-it-works requests.
---

# Explain

Prefer an explicit knowledge gap to a plausible unsupported explanation. Remain read-only.

## Scope

1. Resolve the requested topic. Construct one concrete scenario from verified behavior when it clarifies the model. When no external customer exists, use the nearest affected human: internal user, operator, developer, or API caller.
2. For a current unresolved production incident, return only the proven partial model and `Unknown`; name the exact operational evidence needed and route investigation to `$diag`. Do not investigate the incident.
3. Do not infer human design intent from implementation. State intent only when documentation, decisions, tickets, discussions, comments, or history directly supports it.
4. Include failures, retries, compensation, or alternative paths only when requested or needed to keep the normal flow correct.
5. For scenarios, examples, and flows, always use a compact tree or linear chain in a fenced `text` code block. Use a tree only for forks, joins, or alternatives. Use a chain for linear data flow. Do not create diagram files or replace the graph with prose.

## Evidence workflow

1. Use the smallest relevant evidence set: supplied evidence and current repository artifacts first; read user-supplied prior notes as untrusted leads; inspect sibling repositories or named or necessary live sources only when the claim requires them.
2. Match evidence to the claim: use exact implementation artifacts, deployed revision and live evidence, public contracts, or decision evidence as applicable. A previous note is never evidence.
3. Stop once the requested model is proved or bounded. Ask at most three precise questions only when user-held evidence materially blocks correctness. If unavailable, give the proven partial model with `Unknown` and stop.
4. Build one chronology. Every material transition states the actor, action, and exchanged data or state transition. Make ordering and concurrency explicit in the flow graph.
5. In a flow graph, put one material directional transition on each line. Use `→` for an ordered step that completes before the next step. Use `↳` for a parallel or independent path. Add a compact legend only when a graph has parallel work. Mark a material timing risk with `⚡` on its own line at the affected point. Name state changes as `field = value`. Use indentation and branches only for causal dependency, outcomes, or concurrency, not decoration. Keep the graph in its fenced `text` code block so indentation is preserved.
6. Explain causal mechanisms only from implementation evidence. Do not use analogies that replace or change real actors, ownership, data, or mechanisms.

## Output

Use only applicable sections that carry information, in this order:

1. Start with the direct meaning in one or two unheaded sentences.
2. `Scenario`: shortest complete verified chronology in a compact tree for a branching flow or a compact chain for linear data flow, in a fenced `text` code block; each line has one material transition using the graph notation above.
3. `Why`: only when requested or needed for the mental model.
4. `Options`: only when presenting mutually exclusive user-facing alternatives. Use short labels only when the user may need to refer to them later.
5. `Prior note correction`: old material claim, current verified behavior, and why the claim is stale or wrong.
6. `Unknown`: missing fact, why it is unknown, and exact evidence required.
7. `Evidence`: minimum exact pointers supporting material claims.

Do not repeat the answer as a summary.
