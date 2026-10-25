#!/usr/bin/env bash
# Checks our lexicons (lexicons/computer/) with goat (https://github.com/bluesky-social/goat):
# `lint` for their syntax and style, at error level, and `breaking` for changes to published ones
# that break lexicon evolution rules (it reads the published schemas over the network). goat
# doesn't know the spaces alpha yet (the `space` type, and `space` permissions), so the space/
# lexicons and the permission set that grants them are left out of both. The other authorities'
# lexicons beside ours are theirs to check: `pull` fetches them again as they publish them now,
# plus any NSIDs named after it (see lexicons/README.md).
# It runs goat's pinned version with `go run`, so it needs Go 1.27 or newer.
# Usage: tools/lexicons.sh lint|breaking|pull [nsid...]
set -euo pipefail
cd "$(dirname "$0")/.."

lexicons=()
while IFS= read -r file; do lexicons+=("$file"); done < <(git ls-files --cached --others --exclude-standard -- ':(glob)lexicons/computer/**/*.json' \
  ':(exclude)lexicons/computer/localnetwork/space/*' ':(exclude)lexicons/computer/localnetwork/permissions.json')

goat() { go run github.com/bluesky-social/goat@v0.2.6 "$@"; }

case "${1:-}" in
  lint) goat lex lint --lint-level error "${lexicons[@]}" ;;
  breaking) goat lex breaking "${lexicons[@]}" ;;
  pull)
    shift
    others=()
    while IFS= read -r file; do
      nsid=${file#lexicons/}
      nsid=${nsid%.json}
      others+=("${nsid//\//.}")
    done < <(git ls-files --cached --others --exclude-standard -- ':(glob)lexicons/**/*.json' ':(exclude)lexicons/computer/**')
    goat lex pull --update --lexicons-dir lexicons "${others[@]}" "$@"
    ;;
  *)
    echo "usage: $0 lint|breaking|pull [nsid...]" >&2
    exit 2
    ;;
esac
