#!/bin/sh
# Regenerates the OneLibrary fixtures with the sqlcipher CLI (brew install sqlcipher), an
# implementation independent of locallibrary's, using SQLCipher 4's defaults as rekordbox does.
#
#   empty.db          src/onelibrary/schema.sql only (what locallibrary creates new databases from)
#   wal.db, .db-wal   wal.sql: one track in the database file, a second one only in the WAL
#                     (copied while the connection is open, before SQLite checkpoints on close)
set -eu
cd "$(dirname "$0")"
SCHEMA=../../../src/onelibrary/schema.sql
KEY=r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

sqlcipher "$TMP/empty.db" "PRAGMA key = '$KEY';" ".read $SCHEMA" > /dev/null
cp "$TMP/empty.db" empty.db

sqlcipher "$TMP/wal.db" "PRAGMA key = '$KEY';" "PRAGMA journal_mode = WAL;" ".read $SCHEMA" ".read wal.sql" \
  ".shell cp $TMP/wal.db wal.db; cp $TMP/wal.db-wal wal.db-wal" > /dev/null
