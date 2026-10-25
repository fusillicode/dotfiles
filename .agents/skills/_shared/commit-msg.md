# Commit Message Contract

1. Read the applicable repository commit-message rules and the commits unique to the current branch or change request before choosing a message.
2. Apply this precedence:
   1. Repository-mandated structure, allowed types, and exact scope rules.
   2. Exact user-requested message or format for anything the repository leaves open.
   3. Consistent convention established by commits unique to the current change request or stack for anything the repository leaves open.
   4. Canonical ticket identifier established by the change request, stack, or branch.
   5. Repository-wide recent history as fallback.
3. Treat repository examples as examples, not exact scope mandates.
4. Stop and report the conflict when an exact user request violates a mandatory repository rule.
5. Preserve a consistent ticket or component scope within one change request or stack. Never switch scope style without an explicit requirement or a concrete reason. Stop if the correct scope remains materially ambiguous.
6. Choose the type from the actual change, keep the description concise and imperative, and validate the complete message against repository rules before committing.
