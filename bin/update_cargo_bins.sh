#!/usr/bin/env bash

set -euo pipefail

# cargo CLI tools ❤️
cargo install --force \
  atuin \
  cargo-make \
  ccase \
  difftastic \
  drill \
  fd-find \
  mise \
  pv \
  ripgrep \
  sd \
  sqlx-cli \
  typos-cli \
  watchexec-cli
