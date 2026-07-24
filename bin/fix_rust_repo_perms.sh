#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"
# shellcheck source=../log.sh
source "$script_dir/../log.sh"

usage() {
  printf 'Usage: %s [--clean] [--jobs N] DIRECTORY\n' "${0##*/}" >&2
}

die_usage() {
  usage
  die "$1"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

canonical_directory() {
  (
    cd -P -- "$1"
    pwd
  )
}

repository_seen() {
  local candidate="$1"
  local known

  while IFS= read -r -d '' known; do
    [[ "$known" == "$candidate" ]] && return 0
  done <"$repositories_file"

  return 1
}

record_repository() {
  local candidate="$1"

  if ! repository_seen "$candidate"; then
    printf '%s\0' "$candidate" >>"$repositories_file"
    discovered_repositories=$((discovered_repositories + 1))
  fi
}

workspace_seen() {
  local candidate="$1"
  local known

  while IFS= read -r -d '' known; do
    [[ "$known" == "$candidate" ]] && return 0
  done <"$cargo_workspaces_file"

  return 1
}

record_workspace() {
  local repository="$1"
  local workspace="$2"

  if ! workspace_seen "$workspace"; then
    printf '%s\0' "$workspace" >>"$cargo_workspaces_file"
    printf '%s\0%s\0' "$repository" "$workspace" >>"$manifest_records_file"
    discovered_workspaces=$((discovered_workspaces + 1))
  fi
}

discover_manifests() {
  local manifest
  local manifest_directory
  local repository_root
  local workspace_manifest
  local workspace_directory

  : >"$manifest_paths_file"
  if ! find -P "$target" -type f -name Cargo.toml -print0 >"$manifest_paths_file"; then
    error "Cargo manifest discovery failed below: $target"
    traversal_failed=1
  fi

  while IFS= read -r -d '' manifest; do
    manifest_directory="${manifest%/Cargo.toml}"
    if ! repository_root="$(git -C "$manifest_directory" rev-parse --show-toplevel 2>/dev/null)"; then
      warn "Skipping Cargo manifest outside a Git repository: $manifest"
      continue
    fi

    repository_root="$(canonical_directory "$repository_root")"
    record_repository "$repository_root"
    discovered_manifests=$((discovered_manifests + 1))

    if ((clean_cargo)); then
      if workspace_manifest="$(cargo locate-project --workspace --message-format plain --manifest-path "$manifest" 2>/dev/null)"; then
        workspace_directory="$(canonical_directory "${workspace_manifest%/Cargo.toml}")"
      else
        workspace_directory="$manifest_directory"
      fi

      record_workspace "$repository_root" "$workspace_directory"
    fi
  done <"$manifest_paths_file"
}

find_path_pattern() {
  local pattern="$1"

  pattern=${pattern//\\/\\\\}
  pattern=${pattern//\*/\\*}
  pattern=${pattern//\?/\\?}
  pattern=${pattern//\[/\\[}
  printf '%s' "$pattern"
}

build_pruned_find_arguments() {
  local repository="$1"
  local nested

  find_arguments=(-P "$repository" "(" -type l)

  while IFS= read -r -d '' nested; do
    [[ "$nested" == "$repository/"* ]] || continue
    find_arguments+=(-o -path "$(find_path_pattern "$nested")")
  done <"$repositories_file"

  # Prune before selecting entries so parent cleanup cannot reach nested repositories.
  find_arguments+=(")" -prune -o)
}

remove_quarantine_attribute() {
  local path="$1"
  local attributes

  if ! attributes="$(xattr "$path" 2>&1)"; then
    error "Could not inspect extended attributes: $path"
    return 1
  fi

  case $'\n'"$attributes"$'\n' in
    *$'\ncom.apple.quarantine\n'*)
      if ! xattr -d com.apple.quarantine "$path"; then
        error "Could not remove com.apple.quarantine: $path"
        return 1
      fi
      ;;
  esac
}

wait_for_oldest_quarantine_job() {
  local pid="${quarantine_pids[0]}"

  if ! wait "$pid"; then
    repository_failed=1
  fi

  quarantine_pids=("${quarantine_pids[@]:1}")
}

remove_quarantine_tree() {
  local repository="$1"
  local path

  build_pruned_find_arguments "$repository"
  : >"$paths_file"
  if ! find "${find_arguments[@]}" -print0 >"$paths_file"; then
    error "Quarantine traversal failed: $repository"
    repository_failed=1
  fi

  quarantine_pids=()
  while IFS= read -r -d '' path; do
    remove_quarantine_attribute "$path" &
    quarantine_pids+=("$!")

    if ((${#quarantine_pids[@]} >= jobs)); then
      wait_for_oldest_quarantine_job
    fi
  done <"$paths_file"

  while ((${#quarantine_pids[@]} > 0)); do
    wait_for_oldest_quarantine_job
  done
}

record_cargo_failure() {
  printf '%s\0' "$1" >>"$cargo_failures_file"
}

repository_cargo_failed() {
  local repository="$1"
  local failed_repository

  while IFS= read -r -d '' failed_repository; do
    [[ "$failed_repository" == "$repository" ]] && return 0
  done <"$cargo_failures_file"

  return 1
}

wait_for_oldest_cargo_job() {
  local pid="${cargo_pids[0]}"
  local repository="${cargo_repositories[0]}"
  local manifest="${cargo_manifests[0]}"

  if wait "$pid"; then
    ok "cargo clean: $manifest"
  else
    error "cargo clean failed: $manifest"
    record_cargo_failure "$repository"
  fi

  cargo_pids=("${cargo_pids[@]:1}")
  cargo_repositories=("${cargo_repositories[@]:1}")
  cargo_manifests=("${cargo_manifests[@]:1}")
}

run_cargo_cleanups() {
  local repository
  local manifest

  cargo_pids=()
  cargo_repositories=()
  cargo_manifests=()

  while IFS= read -r -d '' repository && IFS= read -r -d '' manifest; do
    info "cargo clean: $manifest"
    (cd -- "$manifest" && cargo clean) &
    cargo_pids+=("$!")
    cargo_repositories+=("$repository")
    cargo_manifests+=("$manifest")

    if ((${#cargo_pids[@]} >= jobs)); then
      wait_for_oldest_cargo_job
    fi
  done <"$manifest_records_file"

  while ((${#cargo_pids[@]} > 0)); do
    wait_for_oldest_cargo_job
  done
}

clean_repository() {
  local repository="$1"

  processed_repositories=$((processed_repositories + 1))
  repository_failed=0

  if repository_cargo_failed "$repository"; then
    error "Cargo cleanup failed in: $repository"
    repository_failed=1
  fi

  info "Removing quarantine metadata in: $repository"
  remove_quarantine_tree "$repository"

  if ((repository_failed)); then
    failed_repositories=$((failed_repositories + 1))
    error "Repository cleanup failed: $repository"
  else
    ok "Repository cleanup complete: $repository"
  fi
}

jobs=7
clean_cargo=0
while (($# > 0)); do
  case "$1" in
    --clean)
      clean_cargo=1
      shift
      ;;
    --jobs)
      (($# >= 2)) || die_usage "--jobs requires a positive integer"
      [[ "$2" =~ ^[1-9][0-9]*$ ]] || die_usage "--jobs must be a positive integer"
      jobs="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    -*)
      die_usage "Unknown option: $1"
      ;;
    *)
      break
      ;;
  esac
done

if (($# != 1)); then
  die_usage "Expected exactly one directory argument"
fi

target="$1"
[[ -d "$target" ]] || die "Directory not found: $target"
[[ ! -L "$target" ]] || die "Refusing to use a symbolic-link directory: $target"

require_command git
require_command xattr
require_command find
if ((clean_cargo)); then
  require_command cargo
fi

if ((clean_cargo)); then
  info "Remove quarantine metadata and run cargo clean below: $target"
else
  info "Remove quarantine metadata below: $target"
fi
read -r -p "Continue? [y/N] " confirmation
if [[ "$confirmation" != y && "$confirmation" != Y ]]; then
  info "Cancelled before making changes"
  exit 0
fi

target="$(canonical_directory "$target")"
temporary_directory="$(mktemp -d "${TMPDIR:-/tmp}/clean-rust-repositories.XXXXXX")"
repositories_file="$temporary_directory/repositories"
manifest_paths_file="$temporary_directory/manifest-paths"
manifest_records_file="$temporary_directory/manifest-records"
cargo_workspaces_file="$temporary_directory/cargo-workspaces"
paths_file="$temporary_directory/paths"
cargo_failures_file="$temporary_directory/cargo-failures"
trap 'rm -rf "$temporary_directory"' EXIT

: >"$repositories_file"
: >"$manifest_records_file"
: >"$cargo_workspaces_file"
: >"$cargo_failures_file"
traversal_failed=0
discovered_manifests=0
discovered_repositories=0
discovered_workspaces=0
info "Discovering Cargo manifests below: $target"
discover_manifests
if ((clean_cargo)); then
  info "Found $discovered_manifests Cargo manifest(s) in $discovered_workspaces workspace(s) across $discovered_repositories Git repository/repositories"
else
  info "Found $discovered_manifests Cargo manifest(s) across $discovered_repositories Git repository/repositories"
fi

processed_repositories=0
failed_repositories=0
if ((clean_cargo)); then
  run_cargo_cleanups
fi

while IFS= read -r -d '' repository; do
  clean_repository "$repository"
done <"$repositories_file"

if ((traversal_failed)); then
  error "Repository traversal was incomplete"
elif ((processed_repositories == 0)); then
  info "No Rust repositories found below: $target"
elif ((failed_repositories == 0)); then
  ok "Cleaned $processed_repositories Rust repository(s)"
else
  error "$failed_repositories of $processed_repositories Rust repository(s) failed"
fi

exit "$((failed_repositories > 0 || traversal_failed > 0))"
