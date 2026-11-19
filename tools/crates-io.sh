#!/usr/bin/env bash
# The loclib crates on crates.io, set up the one way CI can't: crates.io takes a crate's first
# version only from a person with a token, and only then lets it trust a workflow for the rest.
#   tools/crates-io.sh          says what setup would do: which crates crates.io doesn't have yet,
#                               which don't trust .github/workflows/loclib-release.yml, and whether
#                               the crates-io environment exists. Changes nothing, needs no token.
#   tools/crates-io.sh setup    does it, and is safe to run again: creates the crates-io
#                               environment (deploys from main only), publishes the crates
#                               crates.io doesn't have yet (dependencies first), then for every
#                               crate adds the workflow as its trusted publisher and turns on
#                               "require trusted publishing", so tokens can't publish it any more.
# setup reads a crates.io API token from CRATES_IO_TOKEN, or asks for it. Make one at
# https://crates.io/settings/tokens/new that expires soon, limited to crates matching `loclib-*`,
# with the scopes publish-new and trusted-publishing; delete it afterwards. The environment needs
# `gh` signed in as a repository admin.
set -euo pipefail
cd "$(dirname "$0")/.."

api=https://crates.io/api/v1
repo=cutenode/localnetwork
workflow=loclib-release.yml
environment="crates-io"
# crates.io asks every client to say who it is.
agent="localnetwork tools/crates-io.sh (https://github.com/$repo)"

# The crates Cargo can publish: the loclib crates, not the CLI, its plugins or loclib-testkit.
mapfile -t crates < <(cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | select(.publish != []) | .name' | sort)

crates_io() { curl -fsS -A "$agent" ${token:+-H "Authorization: $token"} -H 'Content-Type: application/json' "$@"; }
# 200 with the crate, 404 without; anything else stops the script.
published() {
  local status
  status=$(curl -sS -o /dev/null -w '%{http_code}' -A "$agent" "$api/crates/$1")
  case $status in
    200) return 0 ;;
    404) return 1 ;;
    *) echo "crates.io answered $status for $1" >&2; exit 1 ;;
  esac
}
trusts_workflow() {
  crates_io "$api/trusted_publishing/github_configs?crate=$1" | jq -e --arg repo "$repo" --arg wf "$workflow" --arg env "$environment" \
    'any(.github_configs[]; "\(.repository_owner)/\(.repository_name)" == $repo and .workflow_filename == $wf and .environment == $env)' > /dev/null
}

new=()
for crate in "${crates[@]}"; do published "$crate" || new+=("$crate"); done

if [[ ${1:-} != setup ]]; then
  echo "Not on crates.io yet: ${new[*]:-none}"
  have=$(comm -23 <(printf '%s\n' "${crates[@]}") <(printf '%s\n' "${new[@]}") | xargs)
  echo "On crates.io: ${have:-none}"
  if gh api "repos/$repo/environments/$environment" > /dev/null 2>&1; then
    echo "GitHub has the $environment environment"
  else
    echo "GitHub has no $environment environment yet"
  fi
  echo "Run tools/crates-io.sh setup to publish the new ones and have every crate trust $workflow."
  exit 0
fi

token=${CRATES_IO_TOKEN:-}
if [[ -z $token ]]; then
  read -rsp 'crates.io token (publish-new, trusted-publishing, loclib-*): ' token
  echo
fi

# Deployments to the environment only from main, so a branch can't publish.
gh api -X PUT "repos/$repo/environments/$environment" --input - > /dev/null <<'JSON'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
JSON
if ! gh api "repos/$repo/environments/$environment/deployment-branch-policies" --jq '.branch_policies[].name' | grep -qx main; then
  gh api -X POST "repos/$repo/environments/$environment/deployment-branch-policies" -f name=main -f type=branch > /dev/null
fi
echo "$environment environment: deploys from main"

if ((${#new[@]})); then
  # Cargo publishes the workspace's crates in dependency order and waits for each to be on the
  # index; the ones crates.io already has are released by CI instead.
  excluded=()
  for crate in "${crates[@]}"; do
    [[ " ${new[*]} " == *" $crate "* ]] || excluded+=(--exclude "$crate")
  done
  CARGO_REGISTRY_TOKEN=$token cargo publish --workspace --locked "${excluded[@]}"
fi

for crate in "${crates[@]}"; do
  if ! trusts_workflow "$crate"; then
    jq -n --arg crate "$crate" --arg owner "${repo%/*}" --arg name "${repo#*/}" --arg wf "$workflow" --arg env "$environment" \
      '{github_config: {crate: $crate, repository_owner: $owner, repository_name: $name, workflow_filename: $wf, environment: $env}}' |
      crates_io -X POST --data @- "$api/trusted_publishing/github_configs" > /dev/null
  fi
  crates_io -X PATCH --data '{"crate": {"trustpub_only": true}}' "$api/crates/$crate" > /dev/null
  echo "$crate: trusts $workflow in $environment, tokens can't publish it"
done
echo "Delete the token at https://crates.io/settings/tokens"
