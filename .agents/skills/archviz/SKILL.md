---
name: archviz
description: Turn system flows and architecture options into standalone HTML sequence diagrams with optional side-by-side payload diffs.
---

# Archviz

Use conversation facts. Check cited sources for material gaps. Label uncertainty and illustrative payloads.

Copy the [HTML template](assets/visualization.html) for new pages. Replace its sample content with the requested flows, payloads, and options. Include history only when needed. Reuse its controls and styles: black background, white text, left alignment, browser-default fonts.

Use identical `<title>` and `<h1>` text. Include the ticket when available, then a brief subject, ` - `, and the visualization focus. Add useful context as a numbered list. For multiple scenarios, add a Scenarios heading and bulleted links to diagram headings.

Read the [sequence reference](references/sequence.md). Follow the template's label toggle, arrow, and accordion order. Keep the opaque accordion in normal flow from the arrow's left edge to the page's right edge. Use one horizontal scrollbar. Preserve JSON lines and size each block to its longest line. Escape HTML text.

For disclosures or options, use the fixed top-right **Controls** popover. Put Expand all and Collapse all first when applicable, then one checkbox per option labeled `a. slug: brief description`. Omit the colon and description when the slug suffices. Reuse the reading-position helper for bulk actions and option changes, including its fallback when an exchange disappears.

For comparisons, read the [diff reference](references/diffs.md). Default new pages to unchecked options. Preserve rerun defaults unless asked otherwise.

Use the requested path or a descriptive HTML filename. Rerun at the same path unless asked otherwise. For scoped reruns, reuse the page and edit only affected content and required template behavior. Check diagrams and controls with targeted reads. Parse displayed JSON locally and report errors without dumping payloads. Link the file and note material gaps.
