#!/usr/bin/env bash
# release.sh — cut a new den release.
#
#   scripts/release.sh vX.Y.Z
#
# Bumps Cargo.toml, refreshes Cargo.lock, creates releases/vX.Y.Z.md from
# the template, opens it in $EDITOR, commits, tags, and pushes. The CI
# workflow takes over once the tag is on origin.

set -euo pipefail

GIT_NAME="Johannes Erhardt"
GIT_EMAIL="johannes@bearocratic.io"

err() { printf "error: %s\n" "$*" >&2; }
say() { printf "→ %s\n" "$*"; }

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/release.sh vX.Y.Z" >&2
  exit 1
fi

TAG="$1"
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  err "tag must match vX.Y.Z (got: $TAG)"
  exit 1
fi
VERSION="${TAG#v}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ ! -f Cargo.toml ]]; then
  err "Cargo.toml not found at $ROOT"
  exit 1
fi

# Working tree must be clean.
if [[ -n "$(git status --porcelain)" ]]; then
  err "working tree not clean — commit or stash first"
  git status --short
  exit 1
fi

# Must be on main.
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
  err "must be on main (currently on $BRANCH)"
  exit 1
fi

# Tag must not already exist locally or on origin.
if git rev-parse "$TAG" >/dev/null 2>&1; then
  err "tag $TAG already exists locally"
  exit 1
fi
if git ls-remote --tags origin "refs/tags/$TAG" | grep -q "$TAG"; then
  err "tag $TAG already exists on origin"
  exit 1
fi

NOTES="releases/$TAG.md"
if [[ -e "$NOTES" ]]; then
  err "$NOTES already exists"
  exit 1
fi

# Sync with origin so we don't tag a stale commit.
say "pulling latest main…"
git pull --ff-only

# Bump Cargo.toml — first `version = "..."` line.
say "bumping Cargo.toml to $VERSION…"
TMP="$(mktemp)"
awk -v ver="$VERSION" '
  !done && /^version[[:space:]]*=/ {
    sub(/"[^"]+"/, "\"" ver "\"")
    done = 1
  }
  { print }
' Cargo.toml > "$TMP"
mv "$TMP" Cargo.toml

NEW="$(awk -F\" '/^version[[:space:]]*=/ { print $2; exit }' Cargo.toml)"
if [[ "$NEW" != "$VERSION" ]]; then
  err "Cargo.toml bump failed (got: $NEW)"
  exit 1
fi

# Pin the README's `cargo install --tag` example to the new release.
if [[ -f README.md ]]; then
  say "updating README.md cargo install tag to $TAG…"
  TMP="$(mktemp)"
  awk -v tag="$TAG" '
    /cargo install --git https:\/\/github\.com\/bearocratic\/den --tag v[0-9]+\.[0-9]+\.[0-9]+/ {
      sub(/--tag v[0-9]+\.[0-9]+\.[0-9]+/, "--tag " tag)
    }
    { print }
  ' README.md > "$TMP"
  mv "$TMP" README.md
fi

# Refresh Cargo.lock by running cargo check (cheaper than build).
say "refreshing Cargo.lock…"
cargo check --quiet

# Create release notes from the template.
say "creating $NOTES from TEMPLATE.md…"
mkdir -p releases
cp releases/TEMPLATE.md "$NOTES"
TMP="$(mktemp)"
awk -v tag="$TAG" '
  NR == 1 && /^# vX\.Y\.Z/ { print "# " tag; next }
  { print }
' "$NOTES" > "$TMP"
mv "$TMP" "$NOTES"

# Open in editor.
EDITOR_BIN="${VISUAL:-${EDITOR:-vim}}"
say "opening $NOTES in $EDITOR_BIN…"
"$EDITOR_BIN" "$NOTES"

# Refuse to ship the untouched template.
if grep -Fq "_One-line summary of this release._" "$NOTES"; then
  err "$NOTES still contains the template placeholder"
  echo "Edit the file ($NOTES), commit it manually, then tag and push." >&2
  exit 1
fi

echo
say "files staged for the release commit:"
git --no-pager diff --stat -- Cargo.toml Cargo.lock README.md "$NOTES"
echo

read -r -p "commit, tag, and push $TAG? [y/N] " yn
if [[ "$yn" != "y" && "$yn" != "Y" ]]; then
  echo "aborted. nothing committed; your changes are still on disk."
  exit 1
fi

# Commit + tag with the bearocratic identity, regardless of local git config.
git add Cargo.toml Cargo.lock README.md "$NOTES"
git -c user.name="$GIT_NAME" -c user.email="$GIT_EMAIL" \
    commit -m "chore(release): $TAG"
git tag "$TAG"

say "pushing main + $TAG…"
git push origin main
git push origin "$TAG"

echo
echo "✓ pushed $TAG. CI will build, release, and update the homebrew formula."
echo "  Watch:  https://github.com/$(git config --get remote.origin.url | sed -E 's|.*github\.com[:/](.+)\.git|\1|')/actions"
