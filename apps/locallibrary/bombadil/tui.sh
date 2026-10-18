#!/usr/bin/env bash
# Explore `locallibrary tui` with Bombadil (tui.ts) on a throwaway copy of a fixture drive.
#
#   ./tui.sh                 # 2 minutes
#   TIME_LIMIT=30m ./tui.sh  # longer; any of Bombadil's options can follow, e.g. --seed 7
#
# The machine's own drives are hidden (LOCALLIBRARY_VOLUMES), the store is an empty folder,
# `open`/`xdg-open` are stubbed out and the TUI's home and working folder are in the run's own
# folder, so random keys can't reach a real drive, a real store or a browser, and anything they
# write (an export to `~/Music`, say) stays in that folder. A path typed in the Mixes view (with
# no recordings listed, enter asks for one) can still name a folder elsewhere to read recordings
# from. Afterwards the drive copy has to still read: every edit the run made is checked.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=lib.sh
. "$here/lib.sh"
# The Cargo workspace: the repository root.
repo=$(cd "$here/../../.." && pwd)

cargo build --quiet --locked --manifest-path "$repo/Cargo.toml" -p locallibrary-cli
bin="${CARGO_TARGET_DIR:-$repo/target}/debug/locallibrary"

work=$(mktemp -d "${TMPDIR:-/tmp}/locallibrary-bombadil.XXXXXX")
out="${OUTPUT_PATH:-$work/out}"
drive="$work/volumes/BOMBADIL"

fixture_drive "$drive"
mkdir -p "$work/store" "$work/bin" "$work/home"

for cmd in open xdg-open; do
  printf '#!/bin/sh\necho "%s $*" >> "%s/opened.log"\n' "$cmd" "$work" > "$work/bin/$cmd"
  chmod +x "$work/bin/$cmd"
done

export LOCALLIBRARY_VOLUMES="$work/volumes"
export LOCALLIBRARY_STORE="$work/store"
export PATH="$work/bin:$PATH"
export COLORTERM=truecolor

# The run must see the copy and nothing else.
seen=$("$bin" --json drives | grep -c '"root"' || true)
if [ "$seen" != 1 ]; then
  echo "expected to see only $drive, saw $seen drives" >&2
  exit 1
fi

status=0
run_bombadil terminal test \
  --specification "$here/tui.ts" \
  --time-limit "${TIME_LIMIT:-2m}" \
  --quiescence-timeout-ms 50 \
  --exit-on-violation \
  --output-path "$out" --output-path-overwrite \
  "$@" \
  /bin/sh -c 'cd "$1" && HOME="$1/home" exec "$2" tui "$3"' sh "$work" "$bin" "$drive" || status=$?

# Whatever the run wrote to the drive must still read. `doctor` reads it and exits 4 for the
# problems it finds, which it always does here: the copy has no audio files.
for args in "info $drive" "tracks $drive" "playlists $drive" "doctor $drive"; do
  code=0
  # shellcheck disable=SC2086
  "$bin" --json $args > /dev/null 2> "$work/check.err" || code=$?
  if [ "$code" != 0 ] && [ "$args $code" != "doctor $drive 4" ]; then
    echo "after the run, \`locallibrary $args\` failed:" >&2
    cat "$work/check.err" >&2
    status=1
  fi
done
if [ -f "$drive/.locallibrary/edits.json" ]; then
  echo "edits journal: $(grep -o '"op"' "$drive/.locallibrary/edits.json" | wc -l | tr -d ' ') edit(s)"
fi
if [ -f "$work/opened.log" ]; then
  echo "would have opened:" && cat "$work/opened.log"
fi
echo "trace and drive copy: $work"
exit $status
