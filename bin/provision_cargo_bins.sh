#!/usr/bin/env bash
set -euo pipefail

script_dir="${BASH_SOURCE%/*}"
source "$script_dir/../log.sh"

declare -a desired_crates=(
  cargo-auditable
  cargo-audit
  cargo-machete
  cargo-make
  cargo-sort
  cargo-sort-derives
  ccase
  fd-find
  jnv
  mise
  pv
  qj
  ripgrep
  sd
  sqlx-cli
  tree-sitter-cli
  typos-cli
)

cargo_crates() {
  cargo install --list | awk '
    /^[[:alnum:]_][^[:space:]]*[[:space:]]/ {
      print $1
    }
  '
  printf '%s\n' "${desired_crates[@]}"
}

failed=0

while IFS= read -r crate; do
  info "cargo install --force $crate"
  case "$crate" in
    cargo-audit)
      cargo install --force "$crate" --features=fix && ok "$crate installed" || {
        error "$crate failed"
        failed=1
      }
      ;;
    cargo-nextest)
      cargo install --force --locked "$crate" && ok "$crate installed" || {
        error "$crate failed"
        failed=1
      }
      ;;
    *)
      cargo install --force "$crate" && ok "$crate installed" || {
        error "$crate failed"
        failed=1
      }
      ;;
  esac
done < <(cargo_crates | awk '!seen[$0]++')

exit "$failed"
