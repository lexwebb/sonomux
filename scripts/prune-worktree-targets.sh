#!/bin/sh
# Delete the target/ of agent worktrees (.claude/worktrees/*/target) with no build in the last N days
# (default 3). Each worktree keeps its own target/ so parallel agents never share build state; this
# stops abandoned ones piling up (62 of them reached 233 GB in a sibling repo).
# Runs from the SessionStart hook in .claude/settings.json. Usage: scripts/prune-worktree-targets.sh [days]
set -eu
root=$(dirname "$(git -C "$(dirname "$0")" rev-parse --path-format=absolute --git-common-dir)")
days=${1:-3}
for t in "$root"/.claude/worktrees/*/target; do
  [ -d "$t" ] || continue
  # A build writes deps/ (target/<profile>/deps or target/<triple>/<profile>/deps).
  recent=$(find "$t" -maxdepth 3 -type d \( -path "$t" -o -name deps \) -mtime -"$days" -print | head -1)
  [ -n "$recent" ] && continue
  echo "removing $t"
  rm -rf "$t"
done
