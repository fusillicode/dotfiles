#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"

rustup update

mise self-update
mise upgrade

/bin/bash "$script_dir/update_cargo_bins.sh"
/bin/bash "$script_dir/update_brew.sh"
/bin/bash "$script_dir/idt" ~/.dev-tools ~/.local/bin
