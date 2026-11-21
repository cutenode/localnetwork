#!/usr/bin/env bash
# Lays out what CI measured on main for `tools/budgets/cli.ts creep`: <dir>/<order>-<sha>/<area>.json
# from the <artifact> of each first-parent commit since <since> that has one, starting from the
# newest commit before <since> that has one. Run in a clone with main's history up to HEAD. CI keeps
# artifacts 14 days, so <since> can go back a week with room.
#   points.sh <artifact> <since> <dir>
set -euo pipefail
artifact=$1 since=$2 dir=$3
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}

# Each commit's newest artifact of that name, from any run of it (a merge queue group's or a push's).
declare -A ids=()
while read -r id sha; do
  [ -n "${ids[$sha]:-}" ] || ids[$sha]=$id
done < <(gh api --paginate "repos/$repo/actions/artifacts?name=$artifact&per_page=100" \
  --jq '.artifacts[] | select(.expired | not) | "\(.id) \(.workflow_run.head_sha)"')

start=()
for sha in $(git rev-list --first-parent --max-count=20 --before="$since" HEAD); do
  if [ -n "${ids[$sha]:-}" ]; then start=("$sha") && break; fi
done

order=0
mkdir -p "$dir"
for sha in "${start[@]}" $(git rev-list --first-parent --reverse --since="$since" HEAD); do
  [ -n "${ids[$sha]:-}" ] || continue
  out=$dir/$(printf %04d "$order")-$sha
  gh api "repos/$repo/actions/artifacts/${ids[$sha]}/zip" > "$out.zip"
  unzip -q -o "$out.zip" -d "$out"
  rm "$out.zip"
  order=$((order + 1))
done
echo "$order measured commits of $artifact since $since"
