#!/usr/bin/env bash
# Instructions of a frame of the player under cachegrind, for each view <area>'s budget lists: the
# frame test over <frames> frames less a run of none, which leaves out reading the fixture.
# Run from the repository's root, it writes <measured-dir>/<area>.json.
#   frame-instructions.sh <unit-test-binary> <frames> <area> <measured-dir>
set -euo pipefail
unit=$1 frames=$2 area=$3 measured=$4

# The frame test would otherwise write its tui.json over the budgets job's, for one view.
unset LOCALLIBRARY_BUDGETS_OUT
# A binary from the coverage archive would otherwise leave a profile in the checkout.
export LLVM_PROFILE_FILE=/dev/null

instructions() {
  LOCALLIBRARY_TUI_VIEW="$1" LOCALLIBRARY_TUI_FRAMES="$2" valgrind --tool=cachegrind --cache-sim=no \
    --cachegrind-out-file=/dev/null "$unit" --exact tui::frames::tests::frames_allocate_what_their_budgets_say 2>&1 |
    tee /dev/stderr | sed -n 's/.*I *refs: *//p' | tr -d ,
}

view_instructions() {
  many=$(instructions "$1" "$frames")
  none=$(instructions "$1" 0)
  echo "$1 $(((many - none) / frames))"
}

# A view at a time: reading the fixture takes most of a debug binary's run, twice for each view, and
# what's counted doesn't depend on what else the machine is running. SHELLOPTS carries `set -e`.
export SHELLOPTS unit frames
export -f instructions view_instructions
jq -r '.metrics | keys_unsorted[]' "benchmarks/budgets/$area.json" |
  xargs -P 0 -I {} bash -c "view_instructions {}" |
  jq -Rn '[inputs | split(" ") | {(.[0]): (.[1] | tonumber)}] | add' >"$measured/$area.json"
