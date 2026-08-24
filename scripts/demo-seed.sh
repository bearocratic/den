#!/usr/bin/env bash
# Seed a throwaway folder of git repos for the README demo recording.
# Neutral names, staged states — real repos, nothing private. Rerun
# freely; the folder is rebuilt from scratch every time.
set -euo pipefail

DEMO="${1:-/tmp/den-demo}"
REMOTES="$DEMO/.remotes"
export GIT_AUTHOR_NAME="demo" GIT_AUTHOR_EMAIL="demo@example.com"
export GIT_COMMITTER_NAME="demo" GIT_COMMITTER_EMAIL="demo@example.com"

rm -rf "$DEMO"
mkdir -p "$DEMO" "$REMOTES"

new_repo() { # name
  git -C "$DEMO" init -q -b main "$1"
  echo "# $1" > "$DEMO/$1/README.md"
  git -C "$DEMO/$1" add -A
  git -C "$DEMO/$1" commit -qm "init"
}

with_origin() { # name — bare origin so ahead/behind is real
  git init -q --bare "$REMOTES/$1.git"
  git -C "$DEMO/$1" remote add origin "$REMOTES/$1.git"
  git -C "$DEMO/$1" push -qu origin main 2>/dev/null
}

# aurora — clean, tagged, synced
new_repo aurora
git -C "$DEMO/aurora" tag v0.4.0
with_origin aurora

# vector — modified, staged, untracked, ahead
new_repo vector
git -C "$DEMO/vector" tag v0.6.0
with_origin vector
for i in 1 2; do
  echo "change $i" >> "$DEMO/vector/src_$i.rs"
  git -C "$DEMO/vector" add -A
  git -C "$DEMO/vector" commit -qm "feat: change $i"
done
echo "staged" > "$DEMO/vector/staged.rs";   git -C "$DEMO/vector" add staged.rs
echo "wip" >> "$DEMO/vector/README.md"
echo "notes" > "$DEMO/vector/scratch.txt"

# helios — modified, behind origin
new_repo helios
with_origin helios
clone="$DEMO/.helios-writer"
git clone -q "$REMOTES/helios.git" "$clone"
echo "upstream work" >> "$clone/README.md"
git -C "$clone" -c user.name=demo -c user.email=demo@example.com commit -qam "upstream: work"
git -C "$clone" push -q
rm -rf "$clone"
git -C "$DEMO/helios" fetch -q origin
echo "local edit" >> "$DEMO/helios/README.md"

# midas — modified, untracked, ahead, stash
new_repo midas
with_origin midas
echo "wip" >> "$DEMO/midas/README.md"
git -C "$DEMO/midas" stash -q
echo "ahead" >> "$DEMO/midas/main.rs"
git -C "$DEMO/midas" add -A
git -C "$DEMO/midas" commit -qm "feat: ahead"
echo "more wip" >> "$DEMO/midas/README.md"
echo "todo" > "$DEMO/midas/notes.md"

# nova — merge conflict, on a feature branch
new_repo nova
git -C "$DEMO/nova" tag v0.2.1
git -C "$DEMO/nova" checkout -qb fix/queue-drain
echo "branch line" >> "$DEMO/nova/README.md"
git -C "$DEMO/nova" commit -qam "fix: branch side"
git -C "$DEMO/nova" checkout -q main
echo "main line" >> "$DEMO/nova/README.md"
git -C "$DEMO/nova" commit -qam "fix: main side"
git -C "$DEMO/nova" checkout -q fix/queue-drain
git -C "$DEMO/nova" merge main >/dev/null 2>&1 || true

# citadel — untracked only, tagged
new_repo citadel
git -C "$DEMO/citadel" tag v0.1.0
echo "draft" > "$DEMO/citadel/draft.md"
echo "sketch" > "$DEMO/citadel/sketch.md"

echo "seeded $DEMO"
