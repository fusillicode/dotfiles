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

if [[ "$mode" == fix ]]; then
  while IFS= read -r -d '' ds_store_file; do
    rm -f "$ds_store_file" || _die "failed removing $ds_store_file"
  done < <(
    find "$repo_root" \
      \( -name .git -o -name target -o -name 'target-*' \) -type d -prune -o \
      -type f -name .DS_Store -print0
  )
fi

has_rust_changes=0
while IFS= read -r status_line; do
  case "$status_line" in
    *".rs"|*"Cargo.toml")
      has_rust_changes=1
      break
      ;;
  esac
done < <(git -C "$repo_root" status --short --untracked-files=all)

if [[ "$has_rust_changes" -eq 0 ]]; then
  exit 0
fi

# Build tec (release for speed on repeated invocations)
cargo build --release --quiet --manifest-path "$workspace_dir/Cargo.toml" --bin tec || _die 'build failed'

[[ -x "$tec" ]] || _die "tec binary missing: $tec"

if [[ "$mode" == fix ]]; then
  "$tec" --fix || _die 'tec --fix failed'
else
  "$tec" || _die 'tec failed'
fi

exit 0
