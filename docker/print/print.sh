#!/bin/sh
# Print command invoked by the domserver. DOMjudge replaces `[file]` in the
# configured command with the path to the file the team requested to print,
# then exec's argv directly (no shell). We append a delimited copy to the
# shared log volume and also echo to stdout so the admin UI shows what was
# captured.

set -eu

LOG=/var/log/domjudge/print.log
SRC="$1"

mkdir -p "$(dirname "$LOG")"
{
    printf '=== printed at %s ===\n' "$(date -u +%FT%TZ)"
    cat "$SRC"
    printf '\n=== end ===\n'
} >> "$LOG"

# Stdout is captured by the printing UI for display back to the admin.
cat "$SRC"
