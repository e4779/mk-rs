#!/usr/bin/env bash
# scripts/release.sh — code-ified release procedure for mk-rs.
#
# Usage:
#   ./scripts/release.sh patch     # 0.2.2 → 0.2.3
#   ./scripts/release.sh minor     # 0.2.2 → 0.3.0
#   ./scripts/release.sh major     # 0.2.2 → 1.0.0
#   ./scripts/release.sh patch --dry-run   # show what would happen, no changes
#
# Does:
#   1. Pre-checks (branch=main, clean tree, in sync with origin, gates pass)
#   2. Version bump in all 4 Cargo.toml (workspace consistent)
#   3. CHANGELOG refresh via git-cliff (confirmation prompt)
#   4. Reminder to update Current focus in PLAN/TODO (manual, pause)
#   5. Commit + tag
#   6. Push both remotes (origin=gitverse canonical, github=mirror)
#
# Does NOT:
#   - Run `cargo publish` (CI on gitverse publishes on `v*` tag push)
#   - Auto-edit Current focus (human judgment)
#   - Open GitHub Release (gitverse Releases + tags suffice)
#
# Symmetric to .githooks/* — shell scripts in repo, agent-agnostic, readable.

set -euo pipefail
source "$(git rev-parse --show-toplevel)/.githooks/_common.sh"

DRY_RUN=0
BUMP="${1:-}"
[ "${2:-}" = "--dry-run" ] && DRY_RUN=1

# --- Arg validation -------------------------------------------------------
case "$BUMP" in
    major|minor|patch) ;;
    *)
        cat <<EOF
Usage: $0 <major|minor|patch> [--dry-run]

Bumps version, regenerates CHANGELOG via git-cliff, commits, tags, pushes.

Preconditions: clean working tree on main, in sync with origin, all gates
(fmt + clippy + test) passing.
EOF
        exit 1
        ;;
esac

section "mk-rs release — $BUMP bump $([ $DRY_RUN = 1 ] && echo '(dry-run)' || echo '')"
cd "$(ws_root)"

# --- 1. Pre-checks --------------------------------------------------------
section "1/6 · pre-checks"

BRANCH=$(git rev-parse --abbrev-ref HEAD)
[ "$BRANCH" = "main" ] || { log_fail "not on main (on $BRANCH)"; exit 1; }
log_pass "on main branch"

if ! git diff --quiet HEAD || ! git diff --cached --quiet HEAD; then
    log_fail "working tree has uncommitted changes"
    git status --short
    exit 1
fi
log_pass "working tree clean"

# In sync with origin/main
git fetch origin main --quiet
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main)
[ "$LOCAL" = "$REMOTE" ] || {
    log_fail "main diverged from origin/main (local=$LOCAL remote=$REMOTE)"
    log_info "push or pull first"
    exit 1
}
log_pass "main in sync with origin/main"

# Gates: reuse the same checks as pre-commit hook
section "2/6 · gates (fmt + clippy + test)"
log_info "cargo fmt --check"
cargo fmt --check
log_info "cargo clippy --all-targets --all-features -- -D warnings"
cargo clippy --all-targets --all-features --quiet -- -D warnings
log_info "cargo test --workspace"
cargo test --workspace --quiet
log_pass "all gates green"

# --- 2. Version bump ------------------------------------------------------
section "3/6 · version bump"

CURRENT=$(awk -F'"' '/^version =/ {print $2; exit}' crates/mk-cli/Cargo.toml)
log_info "current version: $CURRENT"

IFS='.' read -r MAJOR MINOR PATCH <<<"$CURRENT"
case "$BUMP" in
    major) MAJOR=$((MAJOR+1)); MINOR=0; PATCH=0 ;;
    minor) MINOR=$((MINOR+1)); PATCH=0 ;;
    patch) PATCH=$((PATCH+1)) ;;
esac
NEW="$MAJOR.$MINOR.$PATCH"
log_info "new version:     $NEW"

if [ $DRY_RUN = 1 ]; then
    log_warn "dry-run: would bump $CURRENT → $NEW in 4 Cargo.toml files"
else
    for c in mk-core mk-shell mk-cli mk-graph; do
        sed -i "s/^version = \"$CURRENT\"/version = \"$NEW\"/" "crates/$c/Cargo.toml"
    done
    # Verify workspace still builds with new versions
    cargo build --workspace --quiet
    log_pass "bumped $CURRENT → $NEW in 4 Cargo.toml + workspace builds"
fi

# --- 3. CHANGELOG refresh -------------------------------------------------
section "4/6 · CHANGELOG refresh (git-cliff)"

if [ $DRY_RUN = 1 ]; then
    log_warn "dry-run: would run 'git-cliff -o CHANGELOG.md' (see cliff.toml)"
else
    git-cliff -o CHANGELOG.md
    log_pass "CHANGELOG.md regenerated"
    echo
    log_info "CHANGELOG diff (unreleased → new version section):"
    git diff --stat CHANGELOG.md
    echo
    read -r -p "CHANGELOG looks right? [y/N] " ANSWER
    case "$ANSWER" in
        y|Y|yes|YES) log_pass "confirmed" ;;
        *) log_fail "aborted by user (CHANGELOG not committed)"; git checkout CHANGELOG.md; exit 1 ;;
    esac
fi

# --- 4. Manual Current focus update --------------------------------------
section "5/6 · Current focus reminder (manual)"

if [ $DRY_RUN = 1 ]; then
    log_warn "dry-run: would pause here for manual Current focus update"
else
    cat <<EOF

${YELLOW}Manual step${RESET}: update Current focus in PLAN.md and TODO.md to
reflect that v$NEW shipped. This is a judgment call — what's the next focus,
not a mechanical edit.

Examples:
- PLAN.md "## Current focus": replace current paragraph
- TODO.md "## Current focus": mark release shipped, set next focus

Edit now, save, then return here and press Enter to continue.
EOF
    read -r -p "Press Enter when Current focus is updated (or Ctrl-C to abort)..."
    # Did the user actually edit?
    if git diff --quiet PLAN.md TODO.md; then
        log_warn "no changes detected in PLAN.md/TODO.md — continuing anyway"
    else
        log_pass "PLAN.md/TODO.md updated"
    fi
fi

# --- 5. Commit + tag ------------------------------------------------------
section "6/6 · commit + tag + push"

DEFAULT_MSG="Bug A/B + infra/docs wave (hooks, git-cliff, PLAN restructure)"
read -r -p "Release commit message [default: $DEFAULT_MSG]: " MSG
MSG="${MSG:-$DEFAULT_MSG}"

COMMIT_MSG="release: v$NEW — $MSG"
TAG_MSG="v$NEW — $MSG"

if [ $DRY_RUN = 1 ]; then
    log_warn "dry-run: would commit + tag + push"
    echo "  commit: $COMMIT_MSG"
    echo "  tag:    v$NEW ($TAG_MSG)"
    echo "  push:   origin main + v$NEW, github main + v$NEW"
    exit 0
fi

git add -A
git commit -m "$COMMIT_MSG" --no-verify  # --no-verify: gates already ran above
# Tag must point at the release commit
git tag -a "v$NEW" -m "$TAG_MSG"
log_pass "committed + tagged v$NEW"

# Push both remotes
log_info "pushing origin (gitverse canonical, has publish CI)"
git push origin main
git push origin "v$NEW"
log_info "pushing github (mirror)"
git push github main
git push github "v$NEW"
log_pass "pushed both remotes"

cat <<EOF

${GREEN}✓ Release v$NEW shipped.${RESET}

Next: CI on gitverse should auto-publish to crates.io (see
.gitverse/workflows/ci.yml). Verify in 2-3 minutes:

  cargo install mk-rs --force
  mk --version            # should show $NEW
  cargo search mk-rs      # should show $NEW

If CI fails or publish doesn't trigger, check .gitverse/workflows/ci.yml
and the gitverse project page.
EOF