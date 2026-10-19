#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$DIR"

echo "=========================================="
echo "   PowerBeamer GitHub Release Publisher   "
echo "=========================================="

# 1. Check git remote
REMOTE_URL=$(git remote get-url origin 2>/dev/null || echo "")
if [ -z "$REMOTE_URL" ]; then
    echo ""
    echo "⚠️  No git remote 'origin' configured yet!"
    read -p "Enter your GitHub repository URL (e.g. https://github.com/ahmadikalkhorani/PowerBeamer.git): " REPO_INPUT
    if [ -n "$REPO_INPUT" ]; then
        git remote add origin "$REPO_INPUT"
        echo "✓ Added remote origin: $REPO_INPUT"
    else
        echo "Aborted: A GitHub remote URL is required."
        exit 1
    fi
else
    echo "✓ Detected GitHub remote: $REMOTE_URL"
fi

# 2. Check for uncommitted changes
if ! git diff-index --quiet HEAD -- 2>/dev/null; then
    echo ""
    echo "Staging and committing current changes..."
    git add -A
    read -p "Commit message [Default: 'Release preparation and auto-updater integration']: " COMMIT_MSG
    COMMIT_MSG=${COMMIT_MSG:-"Release preparation and auto-updater integration"}
    git commit -m "$COMMIT_MSG"
fi

# 3. Read version from Cargo.toml
CARGO_VER=$(grep '^version = ' Cargo.toml | head -1 | cut -d '"' -f2)
DEFAULT_TAG="v${CARGO_VER}"

echo ""
read -p "Release tag to push to GitHub [$DEFAULT_TAG]: " TAG_NAME
TAG_NAME=${TAG_NAME:-$DEFAULT_TAG}

# 4. Create and push tag
echo ""
echo "Creating tag $TAG_NAME..."
git tag -f "$TAG_NAME" -m "Presenter Release $TAG_NAME"

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
echo "Pushing commits to origin/$CURRENT_BRANCH..."
git push -u origin "$CURRENT_BRANCH"

echo "Pushing tag $TAG_NAME to trigger GitHub Actions Release..."
git push -f origin "$TAG_NAME"

echo ""
echo "=========================================="
echo "✓ Release $TAG_NAME pushed successfully!"
echo "GitHub Actions is now automatically building:"
echo "  • macOS Apple Silicon (arm64)"
echo "  • macOS Intel (x86_64)"
echo "  • Linux (x86_64)"
echo "Check progress at: https://github.com/$(git remote get-url origin | sed -e 's/.*github.com[:\/]//' -e 's/\.git$//')/actions"
echo "=========================================="
