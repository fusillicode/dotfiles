#!/usr/bin/env bash

set -euo pipefail

bins_path="$HOME/.local/bin"
yog_release_path="$PWD/target/release"

cargo fmt && \
cargo clippy && \
cargo build --release && \
    rm -f "$bins_path/idt" && \
    ln -s "$yog_release_path/idt" "$bins_path" && \
    rm -f "$bins_path/yghfl" && \
    ln -s "$yog_release_path/yghfl" "$bins_path" && \
    rm -f "$bins_path/yhfp" && \
    ln -s "$yog_release_path/yhfp" "$bins_path" && \
    rm -f "$bins_path/oe" && \
    ln -s "$yog_release_path/oe" "$bins_path" && \
    rm -f "$bins_path/catl" && \
    ln -s "$yog_release_path/catl" "$bins_path" && \
    rm -f "$bins_path/gcu" && \
    ln -s "$yog_release_path/gcu" "$bins_path"
    rm -f "$bins_path/vpg" && \
    ln -s "$yog_release_path/vpg" "$bins_path" && \
    rm -f "$bins_path/try" && \
    ln -s "$yog_release_path/try" "$bins_path" && \
    rm -f "$bins_path/fkr" && \
    ln -s "$yog_release_path/fkr" "$bins_path" && \
    mv "$yog_release_path/librua.dylib" "$yog_release_path/rua.so"
