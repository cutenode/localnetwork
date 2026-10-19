#!/usr/bin/env bash
# The "Protect Main" ruleset as code: .github/rulesets/protect-main.json is what main requires
# before a merge, and tools/policy/ruleset.rego keeps its required checks in step with the
# workflows' passed jobs.
#   tools/ruleset.sh check   compares GitHub's copy with the file (any token that can read the
#                            repository; bypass actors are left out, since only admins see them)
#   tools/ruleset.sh apply   creates or updates GitHub's copy from the file (needs an admin's
#                            token); safe to run again
set -euo pipefail
cd "$(dirname "$0")/.."

file=.github/rulesets/protect-main.json
repo=${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
name=$(jq -r .name "$file")
id=$(gh api "repos/$repo/rulesets" --jq ".[] | select(.name == \"$name\") | .id")

# The fields both copies share, in one order, for comparing.
normalize='{name, target, enforcement, conditions, rules: (.rules | map(if .type == "required_status_checks" then .parameters.required_status_checks |= sort_by(.context) else . end) | sort_by(.type))}'

case "${1:-}" in
  check)
    if [ -z "$id" ]; then
      echo "::error::GitHub has no \"$name\" ruleset: an admin runs tools/ruleset.sh apply"
      exit 1
    fi
    if ! diff -u <(jq -S "$normalize" "$file") <(gh api "repos/$repo/rulesets/$id" | jq -S "$normalize"); then
      echo "::error::GitHub's \"$name\" ruleset differs from $file (above, - is the file): an admin runs tools/ruleset.sh apply"
      exit 1
    fi
    echo "GitHub's \"$name\" ruleset matches $file"
    ;;
  apply)
    if [ -n "$id" ]; then
      gh api --method PUT "repos/$repo/rulesets/$id" --input "$file" > /dev/null
      echo "Updated \"$name\" from $file"
    else
      gh api --method POST "repos/$repo/rulesets" --input "$file" > /dev/null
      echo "Created \"$name\" from $file"
    fi
    ;;
  *)
    echo "usage: $0 check|apply" >&2
    exit 2
    ;;
esac
