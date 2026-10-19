# Shared by tui.sh and report.sh, and the AT Store screenshots (packages/atstore).

# A drive at $1 copied from the fixtures: 12 tracks and a playlist (bigger-export.pdb), with the
# demo export's analysis and settings files.
fixture_drive() {
  local repo
  repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
  mkdir -p "$1"
  cp -R "$repo/crates/locallibrary/tests/fixtures/rekordcrate/demo_tracks/PIONEER" "$1/"
  cp "$repo/vendor/rekordbox-pdb/tests/data/bigger-export.pdb" "$1/PIONEER/rekordbox/export.pdb"
  rm -f "$1/PIONEER/rekordbox/exportExt.pdb"
}

# Seconds in a Bombadil time limit: 90s, 2m, 1h, 1d.
limit_secs() {
  local n=${1%[smhd]}
  case $1 in
    *s) echo "$n" ;;
    *m) echo $((n * 60)) ;;
    *h) echo $((n * 3600)) ;;
    *d) echo $((n * 86400)) ;;
    *) echo "$1" ;;
  esac
}

# Stop process group $1: TERM, then KILL whatever is still in it a few seconds later.
stop_group() {
  kill -TERM -- "-$1" 2> /dev/null || return 0
  sleep 3
  kill -KILL -- "-$1" 2> /dev/null || true
}

# Run Bombadil (from this folder's npm install) with the given arguments. It has hung waiting
# on Chrome long past --time-limit, so it gets two minutes' grace and is then stopped.
#
# It runs in a process group of its own (perl's setpgrp: macOS has no setsid), and stopping
# it stops the whole group. npx, node and the native binary they start don't pass signals on,
# so signalling npx alone would leave Bombadil, and the TUI or Chrome it drives, running.
run_bombadil() {
  local grace=$(($(limit_secs "${TIME_LIMIT:-2m}") + 120))
  (cd "$here" && exec perl -e 'setpgrp; exec @ARGV or die "$ARGV[0]: $!\n"' npx --no-install bombadil "$@") &
  local pid=$! waited=0
  # The group isn't the terminal's, so Ctrl-C doesn't reach it: pass it on.
  trap 'stop_group "$pid"; exit 130' INT TERM
  while kill -0 "$pid" 2> /dev/null; do
    if [ "$waited" -ge "$grace" ]; then
      echo "bombadil still running ${grace}s in; stopping it" >&2
      stop_group "$pid"
      trap - INT TERM
      return 124
    fi
    sleep 1
    waited=$((waited + 1))
  done
  trap - INT TERM
  wait "$pid"
}
