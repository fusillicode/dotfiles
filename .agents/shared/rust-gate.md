# Rust gate

Load only for Rust source, documentation comments, Cargo manifests/lockfiles, build scripts, generated Rust, Rust configuration, or Rust-affecting workspace inputs. Apply after design planning and semantic edits.

## Style

`MUST`, `DO NOT`, `Never`, and `Do not` are hard; other imperatives are defaults. Repository lint rules are authoritative. Read and apply the [fallback lint baseline](rust-fallback-lints.md) only when the repository specifies no lint rules in configuration, guidance, or check commands. Do not supplement repository lint rules with personal lint flags. Respect repository toolchain and compatibility constraints; report conflicts. DO NOT change repository lint config to enforce the personal baseline.

### Production code

1. For new modules, use the Rust 2024 file layout: `name.rs` and `name/child.rs`; DO NOT create `mod.rs`. DO NOT rename existing modules solely for this rule.
2. DO NOT introduce `unsafe`, `unwrap`, or `expect` in production code. An `unsafe` exception requires recorded repository or user authority plus a comment stating the safety invariant.
3. MUST use clear iterators, correct ownership and borrowing, and early returns with `?` where applicable. MUST infer lifetimes unless explicit names improve API/clarity. MUST borrow when ownership is unnecessary; use `Cow<'_, T>` when both paths matter. MUST avoid unnecessary clones; document non-obvious trade-offs.
4. Use clear names and narrow bounds. MUST use `where` clauses for complex or multi-bound constraints; keep simple bounds inline. MUST return `impl Iterator` or `impl Future` when the concrete type is an implementation detail. Keep internal types private unless the API requires exposure.
5. See `rust-design.md` for domain ownership, conversions, wrappers, related values, and invalid states.

### Readability

1. MUST use the clearest correct form for unfamiliar readers, not the shortest.
2. One narrative per function; separate phases (`validate` → `derive` → `effect` → `return`) with one blank line.
3. Inline conditionals only for simple conditions and outcomes. Otherwise use blocks, guards, `?`, or `let ... else`; name non-obvious predicates; keep nesting ≤2.
4. Keep match arms single-purpose; extract arms with branching, multiple effects, or a second task.
5. Name intermediates for domain values, validation, effects, or ownership. Keep chains only for mechanical selection, projection, or collection. DO NOT compress multi-step domain transformations, validation, or effects into chains or nested expressions.
6. Split functions with multiple named tasks, phase comments, or independent state.
7. Comment non-obvious intent, constraints, and trade-offs; not unclear names or control flow.
8. In the final diff review, reread changed functions and simplify dense control flow.

## Verification

Apply [Repo work](repo-work.md) for shared verification, formatting, and final diff review. Additional Rust requirements:

1. Map task changes to affected Rust packages, targets, features, and behavior tests. Read the relevant Rust configuration.
2. Use repository-selected toolchain/configuration, else local default. Without a pin, record `rustc -Vv` and `cargo -V`. Never import either from another repository.
3. Use explicit `-p` and target selectors (e.g. `--lib`, `--bin <name>`, `--test <name>`). Broader targets/features (`--all-targets`, `--all-features`), matrices, or release builds require a concrete impact reason. If scope remains unclear, report a blocker; DO NOT guess.
4. Select test targets before name filters; filters narrow execution, not compilation. Require matching tests. Omit tests only for ordinary non-documentation-comment-only edits.
5. DO NOT add check/build commands when successful lint/tests already provide equivalent compilation coverage.
6. Use a scoped repository formatter command, else `cargo fmt -p <package>` for each affected package. Failure blocks completion.
7. Pass = exact command exits 0 with no warnings. Warnings, skips, timeouts, missing components, or environment failures fail. All executed focused checks must pass before commit, push, or completion.
