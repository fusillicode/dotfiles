# Rust test guidance

Load only when Rust tests change. Follow the repository test framework, toolchain, and focused-check commands.

## Test design

1. MUST write focused tests for non-trivial logic, including branching, validation, transformation, state transitions, errors, and effects. For user-visible behavior, MUST use a high-level or integration test when a public seam exists; unit-test internal branches. Extract setup only when reused or conceptually complex; keep one-use setup local.
2. Use existing `rstest` or `test_case` for combinatorial cases; DO NOT add dependencies solely for this rule.
3. In unit-test modules, standard-library assertion macros may be unqualified, e.g. `assert_eq!`; qualify them for in-scope clashes or when not in scope. MUST use absolute paths for non-standard macros, e.g. `rstest::rstest`. Use `use super::*;` if and only if referencing parent-module items; otherwise omit it. Put actual before expected in equality or comparison assertions.
4. For structs/enums, MUST use complete-value equality (`assert_eq!(actual, expected)`) when possible and semantically suitable; MUST NOT assert selected fields only. Add `Debug`, `PartialEq`, and `Eq` for tests via `#[cfg_attr(test, derive(Debug, PartialEq, Eq))]` when possible; DO NOT widen production derives. If equality is impossible, unsuitable, or unavailable in an integration test, the test MUST use a semantic helper or test-only wrapper and document why. Library `cfg(test)` is inactive in integration tests.
5. Name tests `fn test_<subject>_when_<scenario>_<outcome>()`; make `<outcome>` verb-led. Never use vague names such as `works`, `case1`, or `handles_edge`.
