#!/usr/bin/env bash
# Common logging functions for shell scripts
# Usage: source "path/to/log.sh"

# Avoid re-sourcing
[[ -n "${_LOG_SH_LOADED:-}" ]] && return 0
readonly _LOG_SH_LOADED=1

# Timestamp helper
_log_timestamp() { date "+%Y-%m-%d %H:%M:%S"; }

# Logging functions with timestamp and colored output
info() { printf "\033[0;90m%s\033[0m \033[0;34m[INFO]\033[0m %s\n" "$(_log_timestamp)" "$1"; }
ok() { printf "\033[0;90m%s\033[0m \033[0;32m[OK]\033[0m %s\n" "$(_log_timestamp)" "$1"; }
warn() { printf "\033[0;90m%s\033[0m \033[0;33m[WARN]\033[0m %s\n" "$(_log_timestamp)" "$1"; }
error() { printf "\033[0;90m%s\033[0m \033[0;31m[ERROR]\033[0m %s\n" "$(_log_timestamp)" "$1" >&2; }

# Fatal error - log and exit
die() {
  error "$1"
  exit "${2:-1}"
}
