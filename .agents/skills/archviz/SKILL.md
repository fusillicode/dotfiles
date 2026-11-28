---
name: archviz
description: Turn system flows and architecture options into standalone HTML sequence diagrams with optional side-by-side payload diffs.
---

# Archviz

Use conversation facts. Check cited sources for material gaps. Label uncertainty and illustrative payloads.

Copy the [HTML template](assets/visualization.html) for new pages. Keep its CSS and script. Use its named classes and sample markup for diagrams, JSON payloads, comparisons, and controls. Replace the sample content. Include history only when needed.

Use identical `<title>` and `<h1>` text. Include the ticket when available, then a brief subject, ` - `, and the visualization focus. Add useful context as a numbered list. For multiple scenarios, add a Scenarios heading and bulleted links to diagram headings.

Use fragment links for same-page navigation. Add `target="_blank" rel="noopener"` to links to other pages.

Read the [sequence reference](references/sequence.md) for message and lane markup. Preserve JSON line breaks and escape HTML text.

Use the template's Controls markup for disclosures or options. Include bulk disclosure buttons when applicable. Label each option `a. slug`, with an optional `: brief description`. Start new options unchecked and preserve rerun defaults unless asked otherwise. Read the [diff reference](references/diffs.md) for comparisons.

Rerun at the same path unless asked otherwise. For scoped reruns, reuse the page and edit only affected content and required template behavior. Check diagrams and controls with targeted reads. Parse displayed JSON locally and report errors without dumping payloads. Link the file and note material gaps.
