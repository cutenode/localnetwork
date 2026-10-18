#!/usr/bin/env bash
# Checks the lexicons with goat (https://github.com/bluesky-social/goat): `lint` for their syntax
# and style, at error level, and `breaking` for changes to published ones that break lexicon
# evolution rules (it reads the published schemas over the network). goat doesn't know the
# spaces alpha yet (the `space` type, and `space` permissions), so the space/ lexicons and the
# permission set that grants them are left out of both.
# It runs goat's pinned version with `go run`, so it needs Go 1.27 or newer.
# Usage: tools/lexicons.sh lint|breaking
set -euo pipefail
cd "$(dirname "$0")/.."

lexicons=()
while IFS= read -r file; do lexicons+=("$file"); done < <(git ls-files --cached --others --exclude-standard -- ':(glob)lexicons/**/*.json' \
  ':(exclude)lexicons/computer/localnetwork/space/*' ':(exclude)lexicons/computer/localnetwork/permissions.json')

goat() { go run github.com/bluesky-social/goat@v0.2.6 "$@"; }

case "${1:-}" in
  lint) goat lex lint --lint-level error "${lexicons[@]}" ;;
  breaking) goat lex breaking "${lexicons[@]}" ;;
  *)
    echo "usage: $0 lint|breaking" >&2
    exit 2
    ;;
esac
