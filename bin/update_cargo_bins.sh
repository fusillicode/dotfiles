#!/usr/bin/env bash

set -euo pipefail

# cargo CLI tools ❤️
cargo install --force \
  atuin \
  cargo-machete \
  cargo-make \
  cargo-sort \
  cargo-sort-derives \
  ccase \
  difftastic \
  drill \
  fd-find \
  mise \
  prmt \
  pv \
  ripgrep \
  sd \
  sqlx-cli \
  typos-cli \
  watchexec-cli
