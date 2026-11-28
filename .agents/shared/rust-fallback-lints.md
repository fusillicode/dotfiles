# Fallback Rust lints

- At priority `-1`, deny Clippy groups `all`, `cargo`, `nursery`, and `pedantic`.
- Also deny: `dbg_macro`, `unimplemented`, `todo`, `unreachable`, `panic`, `unwrap_used`, `expect_used`, `indexing_slicing`, `string_slice`, `get_unwrap`, `unwrap_in_result`, `panic_in_result_fn`, `unused_result_ok`, `assertions_on_result_states`, `mem_forget`, `undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`, `unnecessary_safety_doc`, `unnecessary_safety_comment`, `arithmetic_side_effects`, `float_arithmetic`, `lossy_float_literal`, `float_cmp_const`, `rc_mutex`, `allow_attributes`, `allow_attributes_without_reason`, `implicit_clone`, `redundant_else`, `single_match`, `wildcard_enum_match_arm`, `rc_buffer`, `vec_init_then_push`, `format_push_string`, `verbose_file_reads`, `if_then_some_else_none`, and `unseparated_literal_suffix`.
- Deny Rust `warnings`. Allow only `must_use_candidate` and `multiple_crate_versions`; test-only uses of `dbg`, `expect`, indexing or slicing, `panic`, and `unwrap` override only their matching denies.
- DO NOT suppress `clippy::too_many_lines` or `clippy::too_many_arguments`; split/redesign code.
- `-D warnings` alone is insufficient. Use `-F clippy::unwrap_used` for production checks. Preserve test-only allowances without weakening production checks or adding source suppressions.
