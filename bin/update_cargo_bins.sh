#!/usr/bin/env bash

set -euo pipefail

# cargo CLI tools ❤️
cargo install --force \
  cargo-auditable \
  cargo-machete \
  cargo-make \
  cargo-sort \
  cargo-sort-derives \
  ccase \
  fd-find \
  jnv \
  mise \
  pv \
  qj \
  ripgrep \
  sd \
  sqlx-cli \
  tree-sitter-cli \
  typos-cli \
  worktrunk \

cargo install cargo-audit --features=fix
