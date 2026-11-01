#!/usr/bin/env bash
# Checks our lexicons (lexicons/computer/) with goat (https://github.com/bluesky-social/goat):
# `lint` for their syntax and style, at error level, and `breaking` for changes to published ones
# that break lexicon evolution rules (it reads the published schemas over the network). goat
# doesn't know the spaces alpha yet (the `space` type, and `space` permissions), so the space/
# lexicons and the permission set that grants them are left out of both. `lint` also fails on any
# string in our lexicons, those included, without a format, a maxLength or a maxGraphemes: goat only
# reports those at info level, beside lints we don't hold to, and without naming the field. The
# other authorities' lexicons beside ours are theirs to check: `pull` fetches them again as they publish them now,
# plus any NSIDs named after it (see lexicons/README.md).
# It runs goat's pinned version with `go run`, so it needs Go 1.27 or newer.
# Usage: tools/lexicons.sh lint|breaking|pull [nsid...]
set -euo pipefail
cd "$(dirname "$0")/.."

lexicons=()
while IFS= read -r file; do lexicons+=("$file"); done < <(git ls-files --cached --others --exclude-standard -- ':(glob)lexicons/computer/**/*.json' \
  ':(exclude)lexicons/computer/localnetwork/space/*' ':(exclude)lexicons/computer/localnetwork/permissions.json')

goat() { go run github.com/bluesky-social/goat@v0.2.6 "$@"; }

# Each string without a limit, as `file: path.to.field`.
unbounded() {
  git ls-files --cached --others --exclude-standard -z -- ':(glob)lexicons/computer/**/*.json' |
    xargs -0 jq -r 'paths(objects and .type == "string" and ((has("format") or has("maxLength") or has("maxGraphemes")) | not)) as $path
      | "\(input_filename): \($path | map(tostring) | join("."))"'
}

case "${1:-}" in
  lint)
    goat lex lint --lint-level error "${lexicons[@]}"
    found=$(unbounded)
    if [ -n "$found" ]; then
      printf '%s\n' "$found" "These strings need a maxLength (and a maxGraphemes, for text people write) or a format." >&2
      exit 1
    fi
    ;;
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
