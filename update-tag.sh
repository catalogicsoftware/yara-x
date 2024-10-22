#!/bin/bash

set -e

if [ $# -ne 1 ]; then
    echo "Usage: $0 <tag_name>"
    exit 1
fi

TAG_NAME=$1

COMMITS_TO_PICK=(
    "98ffd1c0d2ac92f6e0e4586af10220e2a8b6c10f"
    "20fa8fa7da0fc8248182d2fbc9d0a05a359fa4f2"
)

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)

cleanup() {
    echo "Cleaning up..."
    git checkout $CURRENT_BRANCH
    git branch -D temp_branch_$TAG_NAME 2>/dev/null || true
}

trap cleanup EXIT

echo "Starting tag update process for $TAG_NAME..."

echo "Creating temporary branch from tag..."
git checkout $TAG_NAME
git checkout -b temp_branch_$TAG_NAME

echo "Cherry-picking commits..."
for commit in "${COMMITS_TO_PICK[@]}"; do
    if ! git cherry-pick $commit; then
        (
            # Resolve conflicts (we get conflicts because we deleted github actions and they change sometimes)
            # '-c core.editor=true' is used to avoid opening editor to write commit message
            git rm -r .github/* && git -c core.editor=true cherry-pick --continue
        ) || (
            echo "Failed to cherry-pick commit $commit" && exit 1
        )
    fi
done

echo "Updating tag..."
git tag -d $TAG_NAME
git tag $TAG_NAME
git push origin :refs/tags/$TAG_NAME
git push origin $TAG_NAME

echo "Reverting changes..."
cleanup

echo "Process completed successfully!"
