#!/usr/bin/env bash
# What benchmarks/budgets/' release areas hold, from a release build of the CLI on Linux: the names
# of the packages it links, the size of its binary and archive, and the instructions everyday
# commands and a frame of the player run under cachegrind, which counts them exactly, on the
# 3,886-track fixture. Builds first, which is a no-op when nothing changed since the last run. Run
# from the root of the checkout to measure; a build into another checkout's target sets
# CARGO_TARGET_DIR.
#   measure-release.sh <measured-dir>
set -euo pipefail
measured=$1
target=${CARGO_TARGET_DIR:-target}
mkdir -p "$measured"
# The CLI's package, by its manifest: a base measured in a checkout of its own with this script may
# be from before it was named `locallibrary` (it was `locallibrary-cli`).
cli=$(cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | select(.manifest_path | endswith("/apps/locallibrary/Cargo.toml")) | .name')

# By name, whatever the version, so a new crate shows and a version bump doesn't. Without colour:
# CARGO_TERM_COLOR colours each repeat's `(*)`, which sed then doesn't take off.
cargo tree --color never --locked --target x86_64-unknown-linux-gnu -e normal,no-proc-macro -p "$cli" --prefix none |
  sed 's/ .*//' | sort -u | jq -R -s '{"locallibrary": split("\n")[:-1]}' > "$measured/deps.json"

cargo build --profile dist -p "$cli" --locked
tar -C "$target/dist" -cJf "$measured/archive.tar.xz" locallibrary
jq -n --argjson binary "$(wc -c < "$target/dist/locallibrary")" --argjson archive "$(wc -c < "$measured/archive.tar.xz")" \
  '{"locallibrary": $binary, "archive": $archive}' > "$measured/size-x86_64-unknown-linux-gnu.json"
rm "$measured/archive.tar.xz"

# The commands run on two copies of the fixture, with a home and a store of their own.
scratch=$(mktemp -d)
for drive in LARGE LARGE2; do
  mkdir -p "$scratch/volumes/$drive/PIONEER/rekordbox"
  cp crates/locallibrary/tests/fixtures/rekordcrate/large/export.pdb "$scratch/volumes/$drive/PIONEER/rekordbox/"
done
mkdir -p "$scratch/home"
counted() {
  LOCALLIBRARY_VOLUMES="$scratch/volumes" LOCALLIBRARY_STORE="$scratch/store" HOME="$scratch/home" \
    valgrind --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null "$@" 2>&1 >/dev/null |
    sed -n 's/.*I *refs: *//p' | tr -d ,
}
# Each command's instructions less a run of /bin/true, which leaves out the loader's and libc's start.
# libc's start looks up the CPU's features and caches, which takes some thousands more instructions on
# some runners' CPUs than on others', so a base measured on one runner and a head on another differed by
# that much with the same code (LOC-827). The CPU goes in the log, to tell runners apart.
grep -m1 '^model name' /proc/cpuinfo || true
start=$(counted /bin/true)
instructions() {
  echo $(($(counted "$target/dist/locallibrary" "$@") - start))
}
{
  echo "version $(instructions --version)"
  echo "drive_ls $(instructions drive ls)"
  echo "tracks_filter $(instructions tracks LARGE --filter 'bpm:120-130 -genre:techno')"
  echo "tracks_search $(instructions tracks LARGE --search house)"
  echo "tracks_sort $(instructions tracks LARGE --sort artist,title)"
  echo "diff $(instructions diff LARGE LARGE2)"
  echo "check $(instructions check LARGE LARGE2)"
  echo "doctor $(instructions doctor LARGE)"
} | jq -Rn '[inputs | split(" ") | {(.[0]): (.[1] | tonumber)}] | add' > "$measured/cli.json"

# A frame of the player: the frame test's instructions over many frames, less a run of none, which
# leaves out reading the fixture. It's the release profile, as dist's test binary would take a
# second fat LTO link.
unit=$(cargo test --release -p "$cli" --test unit --locked --no-run --message-format=json-render-diagnostics |
  jq -r 'select(.executable != null and .target.name == "unit") | .executable')
"$(dirname "$0")/frame-instructions.sh" "$unit" 600 tui-instructions "$measured"
