# Rust gate

Load only for Rust source, documentation comments, Cargo manifests/lockfiles, build scripts, generated Rust, Rust configuration, or Rust-affecting workspace inputs. Apply after design planning and semantic edits.

## Style

`MUST`, `DO NOT`, `Never`, and `Do not` are hard; other imperatives are defaults. Repository lint rules are authoritative. Use the lint baseline below only when the repository specifies no lint rules in configuration, guidance, or check commands. Do not supplement repository lint rules with personal lint flags. Respect repository toolchain and compatibility constraints; report conflicts. DO NOT change repository lint config to enforce the personal baseline.

### Fallback lint baseline

- At priority `-1`, deny Clippy groups `all`, `cargo`, `nursery`, and `pedantic`.
- Also deny: `dbg_macro`, `unimplemented`, `todo`, `unreachable`, `panic`, `unwrap_used`, `expect_used`, `indexing_slicing`, `string_slice`, `get_unwrap`, `unwrap_in_result`, `panic_in_result_fn`, `unused_result_ok`, `assertions_on_result_states`, `mem_forget`, `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`, `unnecessary_safety_doc`, `unnecessary_safety_comment`, `arithmetic_side_effects`, `float_arithmetic`, `lossy_float_literal`, `float_cmp_const`, `rc_mutex`, `allow_attributes`, `allow_attributes_without_reason`, `arbitrary_source_item_ordering`, `implicit_clone`, `redundant_else`, `single_match`, `wildcard_enum_match_arm`, `rc_buffer`, `vec_init_then_push`, `format_push_string`, `verbose_file_reads`, `if_then_some_else_none`, and `unseparated_literal_suffix`.
- Deny Rust `warnings`. Allow only `must_use_candidate` and `multiple_crate_versions`; test-only uses of `dbg`, `expect`, indexing or slicing, `panic`, and `unwrap` override only their matching denies.
- `source-item-ordering` = `["module"]`: `extern_crate` → `use` → `foreign_mod`/`mod` → `global_asm` → `const`/`static` → `ty_alias` → `macro`/`enum`/`struct`/`union`/`trait`/`trait_alias`/`impl`/`fn`.
- DO NOT suppress `clippy::too_many_lines` or `clippy::too_many_arguments`; split/redesign code.

### Locality order

On conflict, keep impl adjacency; then visibility, then caller order.
At module scope, apply item-group order; at module and associated-item scopes, apply:

1. Order by visibility: `pub` → `pub(crate)` → restricted → private.
2. For each type defined in the file, place inherent `impl` blocks immediately after the type, then trait `impl` blocks.
3. Put public APIs before private helpers. Place private helpers after their earliest caller; place shared helpers after their earliest public caller. Keep mutually recursive helpers adjacent. Preserve existing source order for remaining ties.

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
8. Before completion, reread changed functions and simplify dense control flow.

## Verification

Delegate long-running or multi-command execution to `check_runner` with exact commands, pinned toolchain, and a writable log directory. The parent selects checks and accepts results.

1. Map task changes to affected packages, targets, features, and behavior tests; include dependents only when affected. Read relevant repository check commands, toolchain pins, and Rust config.
2. Use repository-selected toolchain/configuration, else local default. Without a pin, record `rustc -Vv` and `cargo -V`. Never import either from another repository.
3. Use explicit `-p` and target selectors (e.g. `--lib`, `--bin <name>`, `--test <name>`). Broader targets/features (`--all-targets`, `--all-features`), matrices, or release builds require a concrete impact reason. DO NOT run repository-wide checks locally; CI owns them. Replace broad task wrappers with equivalent focused commands. If scope remains unclear, report a blocker; DO NOT guess.
4. Follow repository lint rules; otherwise explicitly enable the fallback baseline. For the fallback, `-D warnings` alone is insufficient; use `-F clippy::unwrap_used` for production checks. Preserve test-only allowances without weakening production checks or adding source suppressions.
5. Select test targets before name filters; filters narrow execution, not compilation. Require matching tests. Omit tests only for ordinary non-documentation-comment-only edits.
6. Reuse successful checks across phases while relevant inputs/environment remain unchanged; rerun only those affected by later semantic edits. DO NOT add check/build commands when successful lint/tests already provide equivalent compilation coverage.
7. After checks, format affected packages with repository config; use a scoped repository command, else `cargo fmt -p <package>` for each affected package. Failure blocks completion; formatter-only edits need no rerun.
8. Pass = exact command exits 0 with no warnings. Warnings, skips, timeouts, missing components, or environment failures fail. All executed focused checks must pass before commit, push, or completion. DO NOT claim unexecuted checks passed.
