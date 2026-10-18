#!/usr/bin/env bash
# Checks the repository against the conventions in .github/NEW_PROJECT.md: every app, crate and
# plugin registered where it has to be. The policies are Rego in tools/policy/, run by conftest
# over the files below; OPA checks their format and runs their tests (*_test.rego), which have to
# cover every line. Both run from pinned images, so this needs Docker, as CI's repo-checks.yml does.
set -euo pipefail
cd "$(dirname "$0")/.."

conftest=openpolicyagent/conftest:v0.71.1@sha256:0e77cdf7c1fcde035f3c579e4aaa6664ee2a8be5777eaa9af9f520cdd02280c0
opa=openpolicyagent/opa:1.21.2@sha256:7459fa69661242c8983d6e4116779550cbb17608f1110ddcd4e9e24da4dcb0e5

# Tracked files and new ones not yet added (but not ignored ones), so a new app is checked too.
list() { git ls-files --cached --others --exclude-standard "$@"; }

files=()
while IFS= read -r file; do files+=("$file"); done < <(list -- package.json pnpm-workspace.yaml Cargo.toml lefthook.yml \
  ':(glob)apps/*/package.json' ':(glob)apps/*/Cargo.toml' ':(glob)apps/*/Dockerfile' ':(glob)apps/*/railway.json' ':(glob)packages/*/package.json' \
  ':(glob)crates/*/Cargo.toml' ':(glob)plugins/*/Cargo.toml' ':(glob)vendor/*/Cargo.toml' \
  ':(glob).github/workflows/*.yml' .github/labels.yml .github/labeler.yml ':(glob).github/ISSUE_TEMPLATE/*.yml' \
  ':(glob).github/rulesets/*.json' ':(glob)lexicons/**/*.json')

# Every file, for the policy that each one gets a label.
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
list | jq -R -s '{files: split("\n")[:-1]}' > "$scratch/tracked-files.json"

run() {
  docker run --rm -v "$PWD:/repo:ro" -v "$scratch:/scratch:ro" -w /repo "$@"
}

run "$opa" fmt --list --fail tools/policy
run "$opa" check --strict tools/policy
run "$opa" test tools/policy
uncovered=$(run "$opa" test --coverage tools/policy |
  jq -r '.files | to_entries[] | select(.value.not_covered) | "\(.key): lines \([.value.not_covered[] | "\(.start.row)-\(.end.row)"] | join(", ")) not covered by a test"')
if [ -n "$uncovered" ]; then
  echo "$uncovered"
  exit 1
fi
run "$conftest" test --combine --all-namespaces --no-color --policy tools/policy "${files[@]}" /scratch/tracked-files.json
