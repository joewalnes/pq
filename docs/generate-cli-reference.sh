#!/usr/bin/env bash
#
# Generate docs/src/cli-reference.md from `pq --help` output.
# Run this after changing CLI flags to keep docs in sync.
#
# Usage:
#   ./docs/generate-cli-reference.sh           # uses pq from PATH
#   PQ=./target/release/pq ./docs/generate-cli-reference.sh

set -euo pipefail

PQ="${PQ:-pq}"
OUT="$(dirname "$0")/src/cli-reference.md"

# Verify pq is available
if ! command -v "$PQ" &>/dev/null && [ ! -x "$PQ" ]; then
    echo "error: pq not found. Build it first or set PQ=/path/to/pq" >&2
    exit 1
fi

subcommands=(
    view
    info schema stats layout validate
    cat head tail sample count grep
    sql jq
    select slice merge split
    import export
    completions
)

{
    cat <<'HEADER'
# CLI Reference

> **Auto-generated** - do not edit by hand.
> Run `./docs/generate-cli-reference.sh` to regenerate.

## pq

HEADER

    echo '```text'
    NO_COLOR=1 "$PQ" --help
    echo '```'
    echo

    for cmd in "${subcommands[@]}"; do
        echo "## pq $cmd"
        echo
        echo '```text'
        NO_COLOR=1 "$PQ" "$cmd" --help 2>&1 || true
        echo '```'
        echo
    done
} > "$OUT"

echo "Generated $OUT"
