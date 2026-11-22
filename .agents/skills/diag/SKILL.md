---
name: diag
description: Read-only diagnosis of production failures, regressions, or unexpected behavior. Save or resume a diagnostic checkpoint on request.
---

# Diagnose

Keep diagnosis read-only. Bounded read-only evidence queries, including remote telemetry queries, are allowed. A requested local checkpoint is the only write. Capture the working directory at diagnosis start. Stop before any change to the investigated system.

1. Resolve the symptom, environment, time window, service, and available identifiers.
   When telemetry is needed, delegate bounded evidence collection to the `telemetrist` role with the resolved scope. Keep diagnosis in the parent.
2. Start with the smallest discriminating query. Bound routine output, preserve complete errors, and narrow before widening.
3. Correlate only the logs, metrics, traces, deploys, or configuration needed to distinguish active hypotheses. Use identifiers first; timestamp correlation alone is not causation.
4. Separate observed facts from inferences and unknowns. Add hypotheses or ruled-out alternatives when they matter.
5. Redact secrets and unnecessary customer data.

Report the verdict, confidence, scope, and decisive evidence. Add a causal chain, hypotheses, impact, alternatives, or the next smallest read-only check when relevant.

For checkpoint requests, filter [checkpoint instructions](references/checkpoints.md) locally. For save, handoff, or update, read only “Save a checkpoint on request”. For resume, read only “Resume from a checkpoint”. For combined requests, read both.
