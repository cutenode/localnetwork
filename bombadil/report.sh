#!/usr/bin/env bash
# Explore the interactive mix report with Bombadil (report.ts) in headless Chrome.
#
#   ./report.sh                 # 2 minutes
#   TIME_LIMIT=10m ./report.sh  # longer; any of Bombadil's options can follow
#
# Builds a copy of a fixture drive whose tracks are synthetic audio, "records" a set from four
# of them (make-mix.mjs), has `locallibrary mix` identify it and write the report next to the
# set's audio (a page `--html` writes links it by its file name), then serves them. Needs Chrome
# or Chromium; set CHROME to pick one.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=lib.sh
. "$here/lib.sh"
repo=$(dirname "$here")

cargo build --quiet --locked --manifest-path "$repo/Cargo.toml" -p locallibrary-cli
bin="${CARGO_TARGET_DIR:-$repo/target}/debug/locallibrary"

work=$(mktemp -d "${TMPDIR:-/tmp}/locallibrary-report.XXXXXX")
out="${OUTPUT_PATH:-$work/out}"
drive="$work/MIXDRIVE"
mkdir -p "$drive/PIONEER/rekordbox"
cp "$repo/vendor/rekordbox-pdb/tests/data/bigger-export.pdb" "$drive/PIONEER/rekordbox/export.pdb"

export LOCALLIBRARY_VOLUMES="$work/no-volumes"
export LOCALLIBRARY_STORE="$work/store"
"$bin" --json tracks "$drive" | node "$here/make-mix.mjs" "$drive" "$work/site/set.wav"
"$bin" --quiet mix --drive "$drive" --html "$work/site/set.html" "$work/site/set.wav"

# Chrome: $CHROME, else Playwright's headless shell (full Chrome has stalled Bombadil's state
# capture while the report plays audio), else an installed Chrome, else Playwright's Chromium.
chrome="${CHROME:-}"
for c in "$HOME"/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell \
  "$HOME"/.cache/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell; do
  # The newest (they sort oldest first).
  [ -z "${CHROME:-}" ] && [ -x "$c" ] && chrome="$c"
done
if [ -z "$chrome" ]; then
  chrome="$(command -v google-chrome-stable google-chrome chromium chromium-browser 2> /dev/null | head -1 || true)"
fi
if [ -z "$chrome" ] && [ -x "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" ]; then
  chrome="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
fi
if [ -z "$chrome" ]; then
  for c in "$HOME"/Library/Caches/ms-playwright/chromium-*/chrome-mac*/*.app/Contents/MacOS/* \
    "$HOME"/.cache/ms-playwright/chromium-*/chrome-linux*/chrome; do
    [ -x "$c" ] && chrome="$c"
  done
fi
if [ -z "$chrome" ]; then
  echo "no Chrome or Chromium found; set CHROME (or: npx playwright install --only-shell chromium)" >&2
  exit 1
fi
# Launched muted: the report plays the set when a track is clicked.
printf '#!/bin/sh\nexec "%s" --mute-audio "$@"\n' "$chrome" > "$work/chrome"
chmod +x "$work/chrome"
export CHROME="$work/chrome"

node "$here/serve.mjs" "$work/site" > "$work/url" &
server=$!
trap 'kill $server 2> /dev/null || true' EXIT
for _ in $(seq 50); do
  [ -s "$work/url" ] && break
  sleep 0.1
done
url="$(cat "$work/url")/set.html"

status=0
run_bombadil browser test \
  --headless \
  --time-limit "${TIME_LIMIT:-2m}" \
  --exit-on-violation \
  --output-path "$out" --output-path-overwrite \
  "$@" \
  "$url" "$here/report.ts" || status=$?
echo "trace: $out (bombadil browser inspect $out)"
exit $status
