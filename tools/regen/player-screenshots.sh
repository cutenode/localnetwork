#!/usr/bin/env bash
# The player's screenshots in apps/locallibrary/docs/player/ (src/tui/docs/tests.rs checks them). It also
# rewrites player.md's key tables, which merge as usual since the rest of the file is written by hand.
set -euo pipefail
LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary -E 'test(/tui::docs::tests::/)'
