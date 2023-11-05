#!/bin/bash

set -euo pipefail

# cargo bins ❤️
rustup component add rust-analyzer
cargo install --force \
  atuin cargo-make ccase drill fd-find gitui pv ripgrep mise tailspin typos-cli sd sqlx-cli
cargo install --force taplo-cli --all-features