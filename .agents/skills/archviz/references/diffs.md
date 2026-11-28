# Comparison diffs

- Put the exact named baseline in `.payload-current`, followed by `.option-panel` panes in checkbox order. Exclude history unless it defines the baseline. Reuse lettered option slugs in pane headings.
- Omit Current payloads when absent and Current arrows only when the interaction is absent. Mark an entirely new payload `.diff-added`. Label unavailable and partial examples.
- Use `.diff-added`, `.diff-changed`, and `<del class="diff-removed">` for verified JSON differences. Keep full envelopes, matching key order, and removals at their original nesting. Ignore incidental fixture values. Match array entries by verified identity or identity-defining position. Mark unmatched entries in place.
- Mark verified moves and renames `.diff-changed` at the destination. Add old paths as tooltips where useful. Mark new wrapper keys and their children independently. Treat unverified mappings as additions and removals.
- Gate option panes and exchanges with `data-options`. Space-separated keys match any checked option. Leave shared messages unchanged for payload-only changes. Apply diff classes to guarded message variants and payloads independently.
- Split incompatible flows into guarded `alt` operands. Gate each option-specific operand. Give a frame with only gated operands their combined keys. Use `.option-guard` for visible lettered slugs such as `[a. queued]`.
