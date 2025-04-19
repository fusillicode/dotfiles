#!/usr/bin/env bash

set -euo pipefail

bins_path="$HOME/.local/bin"
target_path="$PWD/target"
cargo_build_profile=

is_debug=$([[ "${1:-}" == "--debug" ]] && echo true || echo false)
if $is_debug; then
  target_location="debug"
else
  target_location="release"
  cargo_build_profile="--$target_location"
fi

echo "Installing in $target_location mode"
target_path+="/$target_location"

cargo fmt && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo build $cargo_build_profile

for binary in idt yghfl yhfp oe catl gcu vpg try fkr; do
  rm -f "$bins_path/$binary"
  ln -s "$target_path/$binary" "$bins_path"
done
mv "$target_path/librua.dylib" "$target_path/rua.so"
