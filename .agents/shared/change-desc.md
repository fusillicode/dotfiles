# Change Request Description Contract

1. Read the applicable repository instructions and source evidence before drafting. Ignore repository description templates and structures.
2. Always use this structure with no prose outside it:

```markdown
# Why

1. <bare material reason>

# What

1. <bare material change>
```

3. Include both sections in this order. Every item must change reviewer understanding or a review decision. Delete anything else.
4. Why: explain only the material problem, goal, rationale, decision, risk, rollout, backout, migration, dependency, follow-up, or caveat. Use lean, clear phrases; grammar is optional. Strip articles, filler, qualifiers, repeated context, implementation mechanics, filenames, and commit narration when meaning survives.
5. What: include only material behavior, interfaces, data flow, or key design choices. Use lean, clear phrases; grammar is optional. Strip articles, filler, qualifiers, repeated context, implementation mechanics, filenames, and commit narration when meaning survives.
6. Use sequentially numbered items in both sections. Do not add prose, unordered lists, extra or empty sections, obvious diff details, repeated title text, ticket or issue references, verification claims, or verification results.
