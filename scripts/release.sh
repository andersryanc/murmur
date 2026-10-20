#!/bin/zsh
# Builds Murmur (Release) from the current commit and publishes it as a GitHub Release, for
# installing on other Macs with scripts/get.sh. Needs a clean, pushed tree and `gh auth login`.
#
# Releases are public, so they must be signed with a Developer ID Application certificate, the
# one Apple issues for distributing outside the App Store. Apple Development and self-signed
# builds are for your own Macs only. Set DEVELOPMENT_TEAM in App/Signing.local.xcconfig to the
# team that owns the certificate; the script shows who signed the build and asks before
# publishing.
#
# The build number is the commit count, so each release on main is higher than the last.
# Other Macs keep the app's permissions across updates as long as every release is signed
# with the same certificate.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! security find-identity -v -p codesigning | grep -q '"Developer ID Application: '; then
  echo "No Developer ID Application certificate in your Keychain. Public releases need one" >&2
  echo "(Apple Developer Program); Apple Development and self-signed builds aren't for publishing." >&2
  exit 1
fi

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

APP=$(scripts/build.sh CURRENT_PROJECT_VERSION="$build" MURMUR_SIGN_IDENTITY="Developer ID Application")
codesign --verify --strict "$APP"

# Anyone can read the signer of a download, so make sure it's the one you mean to publish under.
signature=$(codesign -dv --verbose=2 "$APP" 2>&1)
signer=$(sed -n 's/^Authority=//p' <<<"$signature" | head -1)
team=$(sed -n 's/^TeamIdentifier=//p' <<<"$signature")
if [[ $signer != "Developer ID Application: "* ]]; then
  echo "The build is signed by \"$signer\", not a Developer ID Application certificate." >&2
  exit 1
fi
read -r "reply?Publish $tag signed by \"$signer\" (team $team)? [y/N] "
[[ $reply == [yY]* ]] || { echo "Not published."; exit 1; }

zip="build/Murmur.zip"
rm -f "$zip"
ditto -c -k --keepParent "$APP" "$zip"

git fetch -q --tags
previous=$(gh release list --limit 1 --json tagName --jq '.[0].tagName // empty')
notes=$(git log --format='- %s' ${previous:+$previous..}"$commit")
gh release create "$tag" "$zip" --target "$commit" --title "Murmur $version ($build)" --notes "$notes"
echo "Published $tag. On another Mac, install or update with scripts/get.sh (see README)."
