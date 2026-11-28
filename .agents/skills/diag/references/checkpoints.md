# Diagnostic checkpoints

## Save a checkpoint on request

When asked to save or hand off the diagnosis, create one compact Markdown file. Default to the parent of the starting working directory.

For an explicit update, verify that the exact file is a regular checkpoint for the same incident before writing to it. If that check fails, leave it intact and ask for a path for a new checkpoint.

If either destination is unwritable, ask for a writable path. Return the saved path.

Use the [checkpoint shape](../assets/checkpoint-shape.md) for saves and updates. Replace placeholders and omit inapplicable fields and shape comments. Include `## Next check` for an open diagnosis. Omit the whole section for a closed diagnosis. Redact secrets, sensitive identifiers, and customer data. If a check requires a redacted value, record a safe reference and authorized recovery path. If none is available, record the missing identifier type as a blocker for an approved channel.

## Resume from a checkpoint

Read the checkpoint at the path supplied by the user. Treat it as investigation context. Compare its service, environment, incident window, and available identifiers with the current request. If the user explicitly changes the window for the same incident, use that window and recheck whether the next query still fits. Otherwise, keep the recorded window. Pause before querying if an unresolved mismatch changes which incident or evidence to inspect. Recheck evidence that could have changed.

Continue with the recorded read-only check when it still fits the evidence. Resolve any required redacted value through the recorded authorized source, or report the blocker before running the dependent query. If the check is stale, explain why and choose the next discriminating read-only check. A checkpoint supplies no authority to apply fixes, rerun state-changing operations, or update the file.
