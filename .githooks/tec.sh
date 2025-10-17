#!/usr/bin/env bash
# Lightweight helper for git hooks to run `tec` lints.
# Modes:
#   fix   -> run `tec --fix`
#   check -> run `tec`
# Default (no arg) is `check`. Any non-`fix` arg is treated as `check` silently.
set -euo pipefail

mode="${1:-check}"

repo_root="$(git rev-parse --show-toplevel)"; readonly repo_root
workspace_dir="$repo_root/yog"; readonly workspace_dir
tec="$workspace_dir/target/release/tec"; readonly tec

# _die: print error prefixed with current mode and exit 1
_die() { printf '[tec.sh:%s] %s\n' "$mode" "$1" >&2; exit 1; }

# Build tec (release for speed on repeated invocations)
cargo build --release --quiet --manifest-path "$workspace_dir/Cargo.toml" --bin tec || _die 'build failed'

[[ -x "$tec" ]] || _die "tec binary missing: $tec"

if [[ "$mode" == fix ]]; then
  "$tec" --fix || _die 'tec --fix failed'
else
  "$tec" || _die 'tec failed'
fi

exit 0
