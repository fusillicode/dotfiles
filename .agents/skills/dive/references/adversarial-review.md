# Adversarial review

Review only the interview's artifact, and only on the user's request. Use fresh-context subagents with independent, bounded scopes under the applicable delegation rules. Give them the artifact path and relevant evidence. Ask for concrete objections and supporting evidence. If the required reviewers are unavailable, report that the review could not run.

The lead verifies each finding. Resolve it directly when the fix follows from verified evidence or an accepted decision. Use routine defaults when the choice has no material effect on the outcome. If a finding reopens a ready artifact, persist `State: exploring` and append a timestamped `R:` entry with the reason before any repair, even if the repair is immediate.

Record material choices that evidence and accepted decisions leave open under `Open points`. Wait for the user's answer before settling them. Recheck prerequisites for `Next question` and any new decisions. Keep `Next question` first if it still applies. Otherwise, ask about the most consequential decision the user can answer now.

Record accepted trade-offs as decisions. Reapply the readiness criteria after addressing the findings. A review with no material findings leaves a ready artifact ready.

After review, report the current state. Briefly summarize material corrections and what justified them. List all verified open points as statements and persist them in the brief. If user input is needed, end with the next question under the interview rules.
