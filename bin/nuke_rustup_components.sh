#!/usr/bin/env bash

set -u

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../log.sh
source "$script_dir/../log.sh"

usage() {
    die "Usage: $(basename "$0") <component[,component...]>"
}

if [[ $# -ne 1 || -z "$1" || "$1" == ,* || "$1" == *, || "$1" == *,,* ]]; then
    usage
fi

IFS=',' read -r -a components <<<"$1"
for index in "${!components[@]}"; do
    component="${components[$index]}"
    component="${component#"${component%%[![:space:]]*}"}"
    component="${component%"${component##*[![:space:]]}"}"

    if [[ -z "$component" ]]; then
        die "Component list must not contain empty names."
    fi

    if ! [[ "$component" =~ ^[[:alnum:]][[:alnum:]_.-]*$ ]]; then
        die "Invalid component name: $component"
    fi

    components[index]="$component"
done

if ! toolchains="$(rustup toolchain list)"; then
    die "Failed to list installed Rustup toolchains."
fi

removed=0
failed=0
not_installed=0
removed_proxies=0

while IFS= read -r toolchain; do
    [[ -z "$toolchain" ]] && continue

    info "Checking $toolchain..."

    if ! installed_components="$(rustup component list --toolchain "$toolchain" --installed)"; then
        error "Failed to list installed components for $toolchain."
        failed=$((failed + ${#components[@]}))
        continue
    fi

    for component in "${components[@]}"; do
        if grep -Fqx -e "$component (installed)" -e "$component" <<<"$installed_components" || \
            grep -Fq -e "$component-" <<<"$installed_components"; then
            info "Removing $component from $toolchain..."
            if rustup component remove "$component" --toolchain "$toolchain"; then
                removed=$((removed + 1))
                ok "Removed $component from $toolchain."
            else
                failed=$((failed + 1))
                error "Failed to remove $component from $toolchain."
            fi
        else
            not_installed=$((not_installed + 1))
            warn "$component is not installed for $toolchain."
        fi
    done
done < <(awk '{print $1}' <<<"$toolchains")

cargo_bin_dir="${CARGO_HOME:-"$HOME/.cargo"}/bin"
for component in "${components[@]}"; do
    proxy="$cargo_bin_dir/$component"
    rustup_binary="$cargo_bin_dir/rustup"

    if [[ -L "$proxy" && -e "$rustup_binary" && "$proxy" -ef "$rustup_binary" ]]; then
        info "Removing Rustup proxy $proxy..."
        if rm -- "$proxy"; then
            removed_proxies=$((removed_proxies + 1))
            ok "Removed Rustup proxy $proxy."
        else
            failed=$((failed + 1))
            error "Failed to remove Rustup proxy $proxy."
        fi
    fi
done

info "Summary:
  Component installations removed: $removed
  Rustup proxies removed: $removed_proxies
  Components not installed: $not_installed
  Failures: $failed"

if [[ $failed -gt 0 ]]; then
    exit 1
fi
