#!/usr/bin/env bash
# The player's widgets as the website's stories show them, apps/web/src/lib/tui/captures/
# (src/tui/ui/tests/stories.rs checks them).
set -euo pipefail
LOCALLIBRARY_BLESS=1 cargo nextest run -p locallibrary -E 'test(/tui::ui::tests::stories::/)'
