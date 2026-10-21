#!/usr/bin/env bash
# Bump the version in Cargo.toml, commit, tag, and push to trigger the
# GitHub Actions release workflow (.github/workflows/release.yml).
#
# Usage: scripts/release.sh <patch|minor|major|X.Y.Z> [--dry-run]
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

usage() {
    echo "Usage: $0 <patch|minor|major|X.Y.Z> [--dry-run]"
    exit 1
}

BUMP="${1:-}"
DRY_RUN=0
[ "${2:-}" = "--dry-run" ] && DRY_RUN=1
[ -z "$BUMP" ] && usage

run() {
    if [ "$DRY_RUN" = 1 ]; then
        echo "[dry-run] $*"
    else
        "$@"
    fi
}

# 1. Preconditions
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$BRANCH" != "main" ]; then
    echo "✗ Releases must be cut from main (currently on '$BRANCH')."
    exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
    echo "✗ Working tree is not clean. Commit or stash your changes first."
    exit 1
fi
git fetch --quiet origin main --tags
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]; then
    echo "✗ Local main is not in sync with origin/main. Pull or push first."
    exit 1
fi

# 2. Compute the new version
CURRENT=$(grep -m1 '^version = ' Cargo.toml | cut -d '"' -f2)
IFS=. read -r MAJOR MINOR PATCH <<< "$CURRENT"
case "$BUMP" in
    patch) NEW="$MAJOR.$MINOR.$((PATCH + 1))" ;;
    minor) NEW="$MAJOR.$((MINOR + 1)).0" ;;
    major) NEW="$((MAJOR + 1)).0.0" ;;
    *)
        if [[ "$BUMP" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            NEW="$BUMP"
        else
            usage
        fi
        ;;
esac
TAG="v$NEW"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    echo "✗ Tag $TAG already exists."
    exit 1
fi

echo "Releasing $CURRENT → $NEW ($TAG)"
if [ "$DRY_RUN" = 0 ]; then
    read -r -p "Continue? [y/N] " CONFIRM
    [[ "$CONFIRM" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 1; }
fi

# 3. Verify the build and tests before tagging
run cargo test --quiet

# 4. Bump version (first `version =` line is the [package] one) and refresh Cargo.lock
if [ "$DRY_RUN" = 0 ]; then
    sed -i.bak "1,/^version = \".*\"/s/^version = \".*\"/version = \"$NEW\"/" Cargo.toml
    rm -f Cargo.toml.bak
    cargo update --quiet --workspace
fi

# 5. Commit, tag, push
run git add Cargo.toml Cargo.lock
run git commit -m "chore: release $TAG"
run git tag -a "$TAG" -m "PowerBeamer $TAG"
run git push origin main
run git push origin "$TAG"

REPO=$(git remote get-url origin | sed -e 's/.*github.com[:\/]//' -e 's/\.git$//')
echo ""
echo "✓ $TAG pushed. Watch the build at: https://github.com/$REPO/actions"
