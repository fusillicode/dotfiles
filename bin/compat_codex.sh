#!/usr/bin/env bash

set -euo pipefail

script_dir="${BASH_SOURCE%/*}"
# shellcheck source=log.sh
source "$script_dir/../log.sh"

codex_dir="${CODEX_HOME:-"$HOME/.codex"}"
database_path="$codex_dir/logs_2.sqlite"

for command_name in lsof sqlite3; do
  command -v "$command_name" >/dev/null 2>&1 ||
    die "Missing required command: $command_name"
done

[[ -f "$database_path" ]] ||
  die "Codex log database not found: $database_path"

if open_processes=$(lsof "$database_path" 2>&1); then
  die "The Codex log database is open. Close every Codex CLI and app session, then retry: $open_processes"
elif [[ -n "$open_processes" ]]; then
  die "Unable to determine whether the Codex log database is open: $open_processes"
fi

quick_check=$(sqlite3 -readonly "$database_path" 'PRAGMA quick_check;')
[[ "$quick_check" == "ok" ]] ||
  die "Pre-vacuum integrity check failed: $quick_check"

info "Checkpointing the Codex log database"
checkpoint_result=$(sqlite3 "$database_path" 'PRAGMA wal_checkpoint(TRUNCATE);')
ok "Checkpoint complete: $checkpoint_result"

info "Compacting the active Codex log database"
sqlite3 "$database_path" 'VACUUM;'

final_check=$(sqlite3 -readonly "$database_path" 'PRAGMA quick_check;')
[[ "$final_check" == "ok" ]] ||
  die "Post-vacuum integrity check failed: $final_check"

database_stats=$(sqlite3 -readonly "$database_path" \
  'SELECT printf("page_count=%d\nfreelist_count=%d", page_count, freelist_count)
   FROM pragma_page_count(), pragma_freelist_count();')

ok "Compaction complete: $database_stats"
