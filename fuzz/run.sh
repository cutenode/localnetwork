#!/usr/bin/env bash
# Run one fuzz target until it finds a crash or you stop it, with libFuzzer options after the
# target's name: `fuzz/run.sh anlz -max_total_time=300`.
#
# Needs nightly and cargo-fuzz (`rustup toolchain install nightly`, `cargo install cargo-fuzz`).
# Inputs that reach new code are kept in fuzz/corpus/<target>/ (ignored by git, cached by CI).
# The seeds are read where they are: the tests' fixtures, and fuzz/seeds/ for formats whose tests
# keep their examples inline. A crash is saved in fuzz/artifacts/<target>/; replay it with
# `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`.
set -euo pipefail
cd "$(dirname "$0")/.."

usage="usage: fuzz/run.sh <pdb|anlz|mysettings|tracklist> [libFuzzer options]"
target=${1:?$usage}
shift
fixtures=crates/locallibrary/tests/fixtures/rekordcrate
case $target in
  pdb) seeds=("$fixtures/demo_tracks/PIONEER/rekordbox" "$fixtures/empty/PIONEER/rekordbox") ;;
  anlz) seeds=("$fixtures/demo_tracks/PIONEER/USBANLZ") ;;
  mysettings) seeds=("$fixtures/settings") ;;
  tracklist) seeds=(fuzz/seeds/tracklist) ;;
  *)
    echo "$usage" >&2
    exit 2
    ;;
esac

# libFuzzer saves what it finds in the first folder and only reads the others.
mkdir -p "fuzz/corpus/$target"
exec cargo +nightly fuzz run "$target" "fuzz/corpus/$target" "${seeds[@]}" -- "$@"
