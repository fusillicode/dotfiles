#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"

rustup update

"$HOME/.local/bin/idt" "$HOME/.dev-tools" "$HOME/.local/bin"

mise self-update
mise upgrade

/bin/bash "$script_dir/update_brew.sh"
