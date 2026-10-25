# Rust design gate

Load for Rust source, documentation comments, Cargo manifests/lockfiles, build scripts, generated code, Rust config, or workspace inputs. Apply before planning or editing. After semantic edits, apply `rust-gate.md`; if tests change, apply `rust-tests.md`.

## Authority

Public APIs and the requested compatibility boundary override this gate. Without explicit authority, DO NOT change a public API, wire format, schema, storage model, dependency, architecture boundary, security property, or money path. Internal improvements MUST stay small and compatible. These are design decisions, not bans: keep current code when contract, clarity, or compatibility requires it; record why.

## Before editing

Scan affected code for nine categories:

- domain types and invariants;
- repeated domain literals, separated from protocol, SQL, serialization, and fixture values;
- free functions whose behavior may belong to a receiver;
- `From` or `TryFrom` conversions;
- correlated parameters and coupled `Option` or boolean fields;
- one-use forwarding wrappers;
- validation that should yield a validated type;
- internal states, intermediate values, or transitions that permit invalid combinations;
- redundant type paths or aliases.

Review all categories. Record only new or changed candidates in session: location → owner/invariant → decision, with reasons for kept candidates. Report files require explicit requests.

## Design rules

### Domain values

Name repeated domain literals with one owner: a constant, enum variant, or dedicated type. Keep protocol tokens, SQL, serialization names, and one-off fixtures local when context is clearer. Do not make every string a constant.

### Behavior ownership

Use a method when a type owns behavior, data, or an invariant. Keep free functions for orchestration, multi-owner operations, type-neutral transformations, or work with no natural receiver. Do not turn every free function into a method.

### Conversions

Use `From` for a complete infallible conversion and `TryFrom` for fallible validation or conversion. Keep contextual actions, queries, partial projections, parsing, and named domain operations explicit. Do not disguise context as conversion.

### Related values and wrappers

Bundle values that must be valid together in a validated struct, enum variant, newtype, or typestate. Replace a behavior-switching boolean with a named enum or type. Do not add a one-use wrapper that only forwards, renames, or returns one expression. Keep an abstraction only when it adds a qualifying benefit: an invariant, misuse prevention, ownership clarity, domain behavior, an API or trait boundary, reuse, or independent testing.

### Invalid states

MUST make invalid states unrepresentable wherever a type can enforce them: public and module APIs, internal functions and methods, intermediate values, state transitions, and stored representations. Validate once at the earliest trusted boundary, then pass the validated type through every internal seam. Do not pass loosely related primitives or independent `Option` or boolean fields when they permit illegal combinations. Do not reparse or rematch the original request downstream.

### Abstraction level

Use a primitive or plain struct by default. Add a newtype, wrapper, method, or enum only when it provides a qualifying benefit. Never add an abstraction only to satisfy this gate.

### Type paths and aliases

Use short imported names. Qualify only for in-scope same-name clashes. Never qualify for style or add an alias only to shorten a path. For clashes, count references in the containing scope, excluding import declarations; MUST import the more-used type. Alias the other at 2+ uses, else qualify it. Ties use the shorter path, then lexical order.

## Final design review

After semantic edits and again after formatting, inspect the final diff. Classify each candidate as `fix`, `keep with reason`, or `out of scope`:

- **Literals:** One owner per domain value; context-sensitive values stay local.
- **Ownership:** The type owning data or an invariant owns its behavior; orchestration stays free.
- **Conversions:** The right trait for each complete conversion; contextual operations stay named.
- **Wrappers:** Each adds a qualifying benefit.
- **Related values:** Coupled values use one validated representation.
- **Invalid states:** No known illegal combination crosses an internal seam.
- **Type paths and aliases:** Apply the clash rule above.

For new or changed `keep with reason` or `out of scope`, give reason and exact location in session. Test changed behavior and new invariants at the narrowest stable seam. Use `rust-gate.md` for toolchain, formatting, Cargo, and lint; `rust-tests.md` for test changes.
