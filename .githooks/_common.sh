#!/usr/bin/env bash
# Shared helpers for mk-rust git hooks.
# Sourced by .githooks/pre-commit and .githooks/pre-push.
#
# Hooks live in .githooks/ (gitignored from default .git/hooks/).
# Install once per clone:  git config core.hooksPath .githooks

set -uo pipefail

# --- Colors (disabled when output isn't a TTY) ---------------------------
if [ -t 1 ]; then
    RED=$'\e[31m'; GREEN=$'\e[32m'; YELLOW=$'\e[33m'; BLUE=$'\e[34m'
    BOLD=$'\e[1m'; RESET=$'\e[0m'
else
    RED=""; GREEN=""; YELLOW=""; BLUE=""; BOLD=""; RESET=""
fi

log_pass() { printf "%s✓%s %s\n" "$GREEN" "$RESET" "$*"; }
log_fail() { printf "%s✗%s %s\n" "$RED" "$RESET" "$*"; }
log_info() { printf "%s·%s %s\n" "$BLUE" "$RESET" "$*"; }
log_warn() { printf "%s!%s %s\n" "$YELLOW" "$RESET" "$*"; }
section()   { printf "\n%s== %s ==%s\n" "$BOLD" "$*" "$RESET"; }

# --- Bail-out with a hook-friendly exit code -----------------------------
# Exit code 1 = generic block; hooks catch and re-print a friendly message.
hook_die() {
    printf "\n%sHook failed.%s Fix the issues above or bypass with\n" "$RED" "$RESET"
    printf "  %sgit commit --no-verify%s (use sparingly — see CONTRIBUTING.md)\n" "$YELLOW" "$RESET"
    exit 1
}

# --- Detect Cargo workspace root -----------------------------------------
# Hooks may be invoked from a subdirectory; resolve to workspace root.
ws_root() {
    git rev-parse --show-toplevel 2>/dev/null \
        || { log_fail "not inside a git repo"; exit 1; }
}

# --- Cargo profiling: keep output quiet unless CARGO_HOOKS_VERBOSE=1 ----
cargo_quiet_flags() {
    if [ "${CARGO_HOOKS_VERBOSE:-0}" = "1" ]; then
        echo ""
    else
        # --quiet still shows warnings/errors, just not the per-crate chatter.
        echo "--quiet"
    fi
}
