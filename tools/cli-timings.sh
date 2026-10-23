#!/usr/bin/env bash
# Times the locallibrary CLI with hyperfine: starting up, and read-only commands over a real
# 3,886-track rekordbox export (the test fixture in crates/locallibrary/tests/fixtures). With two
# binaries, it times both and fails when the second is much slower than the first at anything.
#
#   tools/cli-timings.sh NEW [OLD]
#
# Prints hyperfine's Markdown tables, or appends them to $GITHUB_STEP_SUMMARY in CI. With
# CLI_TIMINGS_BMF set to a path, it also writes each command's fastest run on the new binary there
# in Bencher's metric format (latency in nanoseconds), which the weekly `cli-timings` job in
# .github/workflows/locallibrary-scheduled.yml sends to Bencher after timing main against main a
# week earlier.
set -euo pipefail

new=$(realpath "$1")
old=${2:+$(realpath "$2")}
root=$(cd "$(dirname "$0")/.." && pwd)
bmf=${CLI_TIMINGS_BMF:-}
[[ -z $bmf || $bmf = /* ]] || bmf=$PWD/$bmf

# A fastest run this much slower fails, unless it's within `floor` seconds. Each binary's runs
# come one after the other, so anything else the machine does lands on one more than the other:
# timing a binary against a copy of itself on a busy Mac, the medians differed by up to 37% and
# the fastest runs by up to 17%, and by a millisecond or two on the quickest commands. The fastest
# run is the one least slowed by the rest of the machine.
ratio=1.25
floor=0.003

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Nothing reads or writes the real store, settings or caches.
export HOME="$work/home" LOCALLIBRARY_STORE="$work/store" NO_COLOR=1
mkdir -p "$HOME"
for drive in usb copy; do
  mkdir -p "$work/$drive/PIONEER/rekordbox"
  cp "$root/crates/locallibrary/tests/fixtures/rekordcrate/large/export.pdb" "$work/$drive/PIONEER/rekordbox/"
done
cd "$work"

# name|arguments. Each reads the whole library: parsing it, then listing, filtering and sorting
# its tracks, comparing it with a copy of itself, and summarising its play history.
commands=(
  "version|--version"
  "help|--help"
  "tracks|-q --no-pager tracks usb"
  "query|-q --no-pager tracks usb --search house --filter bpm:120-128 --sort key,bpm"
  "diff|-q --no-pager diff usb copy"
  "profile|-q --no-pager profile usb"
)

summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
{
  echo "## CLI timings"
  echo
  echo "At least 50 runs of each over a real 3,886-track rekordbox export. A command fails when"
  echo "its fastest run is more than $(jq -n "($ratio - 1) * 100 | round")% and $(jq -n "$floor * 1000 | round") ms slower than the old build's."
  echo
} >>"$summary"

failed=0
for entry in "${commands[@]}"; do
  name=${entry%%|*}
  args=${entry#*|}
  runs=(-n "new" "$new $args")
  # A command the old binary doesn't have (or refuses) is timed on the new one alone.
  # shellcheck disable=SC2086 # the arguments are words
  if [ -n "$old" ] && "$old" $args >/dev/null 2>&1; then
    runs=(-n "old" "$old $args" "${runs[@]}")
  fi
  hyperfine --shell=none --warmup 3 --min-runs 50 --style basic \
    --export-json "$name.json" --export-markdown "$name.md" "${runs[@]}" >&2
  { echo "### $name"; echo; cat "$name.md"; echo; } >>"$summary"
  # hyperfine 2 moved the name and the times (schema_version 2); 1.x has them at the top.
  if verdict=$(jq -r --argjson ratio "$ratio" --argjson floor "$floor" '
      (.results | map({key: (.name // .command), value: (.summary.time_wall_clock.min // .min)})
        | from_entries) as $m
      | select($m.old != null and $m.new > $m.old * $ratio and $m.new - $m.old > $floor)
      | "\($m.new * 1000 | round) ms against \($m.old * 1000 | round) ms"' "$name.json") && [ -n "$verdict" ]; then
    echo "::error::locallibrary $args got slower: its fastest run took $verdict"
    failed=1
  fi
  jq --arg name "$name" '.results[] | select((.name // .command) == "new")
    | {("cli/" + $name): {latency: {value: ((.summary.time_wall_clock.min // .min) * 1e9 | round)}}}' \
    "$name.json" >>bmf.json
done
[ -z "$bmf" ] || jq -s add bmf.json >"$bmf"
exit "$failed"
