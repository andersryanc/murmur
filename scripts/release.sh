#!/bin/zsh
# Builds Murmur (Release) from the current commit and publishes it as a GitHub Release, for
# installing on other Macs with scripts/get.sh. Needs a clean, pushed tree and `gh auth login`.
#
# The build number is the commit count, so each release on main is higher than the last. The
# app keeps your Apple Development signature: other Macs run it as long as it isn't quarantined
# (get.sh downloads with curl, which doesn't quarantine), and keep its permissions across
# updates as long as every release is signed with the same certificate.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -n $(git status --porcelain) ]]; then
  echo "Commit or stash your changes first, so the release matches a commit." >&2
  exit 1
fi
commit=$(git rev-parse HEAD)
if ! git branch -r --contains "$commit" | grep -q .; then
  echo "Push $(git rev-parse --abbrev-ref HEAD) first; the release is tagged at a pushed commit." >&2
  exit 1
fi

build=$(git rev-list --count HEAD)
version=$(sed -n 's/^ *MARKETING_VERSION: "\(.*\)"/\1/p' App/project.yml)
tag="build-$build"
if gh release view "$tag" >/dev/null 2>&1; then
  echo "Release $tag already exists. Commit something new first." >&2
  exit 1
fi

APP=$(scripts/build.sh CURRENT_PROJECT_VERSION="$build")
codesign --verify --strict "$APP"

zip="build/Murmur.zip"
rm -f "$zip"
ditto -c -k --keepParent "$APP" "$zip"

git fetch -q --tags
previous=$(gh release list --limit 1 --json tagName --jq '.[0].tagName // empty')
notes=$(git log --format='- %s' ${previous:+$previous..}"$commit")
gh release create "$tag" "$zip" --target "$commit" --title "Murmur $version ($build)" --notes "$notes"
echo "Published $tag. On another Mac, install or update with scripts/get.sh (see README)."
