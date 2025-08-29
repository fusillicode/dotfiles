AGENTS.md — quick guide for coding agents in this repo

• Project: Rust 2024 workspace (members in src/*). Uses color-eyre for error reporting and a shared utils crate.

Build/lint/test
• Build all: cargo build --workspace
• Build one crate: cargo build -p <crate>
• Run bin: cargo run --bin <name> [-- <args>]
• Test all: cargo test --workspace --all-features
• Test one crate: cargo test -p <crate>
• Run a single test (by name substring): cargo test -p <crate> <test_name> -- --nocapture
• Clippy (strict): cargo clippy --workspace --all-targets -- -D warnings
• Format: cargo +nightly fmt --all (repo uses nightly rustfmt options)

Style/conventions
• Edition: 2024. rustfmt rules in .rustfmt.toml: group_imports=StdExternalCrate, imports_granularity=Item, reorder_*=true, max_width=120, wrap_comments, use_try_shorthand, use_field_init_shorthand. Always run the fmt command above.
• Imports: prefer grouped std/external/crate ordering; granular (Item) imports; keep unused imports out; use full paths or crate:: where appropriate.
• Types/naming: snake_case for functions/variables, CamelCase for types, SCREAMING_SNAKE_CASE for consts. Avoid implicit into()/unwrap(); prefer explicit types at public boundaries.
• Errors: use color_eyre::Result<T> (or eyre::Result) in mains/libs; bubble with ?; enrich context where helpful; avoid panics except in tests or truly unrecoverable paths.
• Testing: place unit tests in mod tests blocks; integration tests per crate if added; use rstest/assert2/pretty_assertions when appropriate (declared in workspace for dev).
• Lints: treat clippy warnings as errors (-D warnings). Keep the tree warning-free.
• Git hygiene: small, focused commits; run fmt + clippy + tests locally before committing.

Notes
• Toolchain: rust-toolchain.toml pins toolchain; nightly required for formatting options.
• External tooling: some binaries expect system tools (curl, gh, git, hx, nvim, tar, vault, wezterm, zcat) — see README.md.
• No Cursor/Copilot rules found in this repo at .cursor/rules/, .cursorrules, or .github/copilot-instructions.md.