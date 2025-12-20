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
  mise \
  pv \
  ripgrep \
  sd \
  sqlx-cli \
  starship \
  typos-cli

cargo install cargo-audit --features=fix
