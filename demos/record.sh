#!/usr/bin/env bash
#
# Record a demo script and convert to GIF.
#
# Usage: ./demos/record.sh <script.py> <output.gif> [cols] [rows]
#
# Environment:
#   PQ           — path to pq binary    (default: ./target/release/pq)
#   ASCIINEMA    — path to asciinema     (default: ~/.local/bin/asciinema)
#   AGG          — path to agg           (default: ~/.local/bin/agg)
#   AGG_THEME    — agg color theme       (default: asciinema)
#   AGG_SPEED    — playback speed        (default: 1)
#   FONT_SIZE    — font size in pixels   (default: 14)

set -euo pipefail

SCRIPT="$1"
OUTPUT="$2"
COLS="${3:-120}"
ROWS="${4:-35}"

PQ="${PQ:-./target/release/pq}"
ASCIINEMA="${ASCIINEMA:-$HOME/.local/bin/asciinema}"
AGG="${AGG:-$HOME/.local/bin/agg}"
AGG_THEME="${AGG_THEME:-asciinema}"
AGG_SPEED="${AGG_SPEED:-2}"
FONT_SIZE="${FONT_SIZE:-14}"

CAST="$(mktemp -t pq-demo-XXXXXX.cast)"
trap 'rm -f "$CAST"' EXIT

echo "==> Recording ${SCRIPT}  (${COLS}x${ROWS})"
PQ="$PQ" "$ASCIINEMA" rec "$CAST" \
    --command "python3 demos/driver.py $SCRIPT" \
    --window-size "${COLS}x${ROWS}" \
    --overwrite \
    --idle-time-limit 2 \
    --output-format asciicast-v2 \
    --quiet

echo "==> Converting → ${OUTPUT}"
"$AGG" \
    --idle-time-limit 2 \
    --speed "$AGG_SPEED" \
    --theme "$AGG_THEME" \
    --font-size "$FONT_SIZE" \
    --line-height 1.0 \
    --last-frame-duration 5 \
    --quiet \
    "$CAST" "$OUTPUT"

echo "==> Done: ${OUTPUT} ($(du -h "$OUTPUT" | cut -f1))"
