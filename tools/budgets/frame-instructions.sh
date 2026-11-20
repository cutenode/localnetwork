#!/usr/bin/env bash
# Instructions of a frame of the player, for each view <area>'s budget lists: what the frame test's
# `frame`, its draw, runs on its thread over <frames> frames, under callgrind. That leaves out reading
# the fixture, the clock its workers' waits poll and the audio each frame reads, which all depend on
# timing. Run from the repository's root, it writes <measured-dir>/<area>.json. The binary needs its
# symbols, to find `frame` by name.
#   frame-instructions.sh <unit-test-binary> <frames> <area> <measured-dir>
set -euo pipefail
unit=$1 frames=$2 area=$3 measured=$4
cachegrind=$(dirname "$0")/cachegrind.sh

# Only the variables given reach the test, so the frame test doesn't write its tui.json over the
# budgets job's, and LLVM_PROFILE_FILE keeps a binary from the coverage archive from leaving a profile
# in the checkout.
view_instructions() {
  instructions=$("$cachegrind" --toggle-collect='*::tui::frames::tests::frame' LOCALLIBRARY_TUI_VIEW="$1" \
    LOCALLIBRARY_TUI_FRAMES="$frames" LLVM_PROFILE_FILE=/dev/null \
    "$unit" --exact tui::frames::tests::frames_allocate_what_their_budgets_say)
  echo "$1 $((instructions / frames))"
}

# A view at a time: reading the fixture takes most of a debug binary's run, and what's counted
# doesn't depend on what else the machine is running. SHELLOPTS carries `set -e`.
export SHELLOPTS unit frames cachegrind
export -f view_instructions
jq -r '.metrics | keys_unsorted[]' "benchmarks/budgets/$area.json" |
  xargs -P 0 -I {} bash -c "view_instructions {}" |
  jq -Rn '[inputs | split(" ") | {(.[0]): (.[1] | tonumber)}] | add' >"$measured/$area.json"
