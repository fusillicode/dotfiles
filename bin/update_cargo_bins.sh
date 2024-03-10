#!/usr/bin/env bash

set -euo pipefail

# cargo CLI tools ❤️
cargo install --force \
  atuin cargo-make ccase drill fd-find gitui pv ripgrep mise tailspin typos-cli sd sqlx-cli
