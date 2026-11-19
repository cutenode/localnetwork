#!/usr/bin/env bash
# What benchmarks/budgets/' pull-request areas hold, from a test archive of the workspace: what
# everyday operations and a frame of the player allocate, what writing a library asks of the
# filesystem, and a frame's instructions in the archive's debug build. Run from the root of the
# checkout the archive was built in, so the paths the counts include are the same for each commit.
#   measure-archive.sh <archive> <measured-dir>
set -euo pipefail
archive=$1 measured=$2

extract=$(mktemp -d)
mkdir -p "$measured"
LOCALLIBRARY_BUDGETS_OUT=$measured cargo nextest run --archive-file "$archive" --extract-to "$extract" \
  -E 'binary(counters) | binary(export_counters) | test(/tui::frames::tests::/)'
# So a pure CPU slowdown shows on the pull request and doesn't wait for the weekly release-profile
# run. Unoptimised and built for coverage, these only compare one build's frame with another's.
"$(dirname "$0")/frame-instructions.sh" "$extract"/target/debug/deps/unit-* 200 tui-debug-instructions "$measured"
