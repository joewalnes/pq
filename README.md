# pq

A Parquet Swiss Army Knife. Inspect, query, transform, and explore Parquet files from the command line.

## 10-second tutorial

```sh
# What's in this file?
$ pq info events.parquet
File:         events.parquet
Size:         1.2 GiB
Rows:         48,291,037
Row Groups:   12
Columns:      8
Compression:  ZSTD
Created by:   pq 0.1.0

# Peek at the data (pretty tables in a terminal, JSONL when piped)
$ pq head events.parquet
┌────┬───────┬─────────┬──────────────────────┬──────────┬──────────┬────────┬──────────┐
│ id │ event │ user_id │ ts                   │ page     │ duration │ active │ city     │
├────┼───────┼─────────┼──────────────────────┼──────────┼──────────┼────────┼──────────┤
│  1 │ click │     402 │ 2025-01-15T08:23:11Z │ /home    │     0.23 │ true   │ Seattle  │
│  2 │ view  │     117 │ 2025-01-15T08:23:14Z │ /pricing │     1.07 │ true   │ Portland │
│  … │       │         │                      │          │          │        │          │
└────┴───────┴─────────┴──────────────────────┴──────────┴──────────┴────────┴──────────┘

# SQL queries — reference files directly in FROM
$ pq sql "SELECT city, count(*) n FROM 'events.parquet' WHERE active GROUP BY city ORDER BY n DESC LIMIT 3"
┌──────────┬────────┐
│ city     │ n      │
├──────────┼────────┤
│ Seattle  │ 12,485 │
│ Portland │  9,712 │
│ Denver   │  8,033 │
└──────────┴────────┘

# jq expressions
$ pq jq events.parquet '{city, event}' | head -2
{"city":"Seattle","event":"click"}
{"city":"Portland","event":"view"}

# Pipe-friendly: JSONL output when stdout isn't a terminal
$ pq cat events.parquet --columns city,event --where "duration > 1.0" | wc -l
23917

# Create parquet from JSON
$ pq convert data.jsonl -o data.parquet

# Interactive TUI explorer
$ pq explore events.parquet
```

## Install

```sh
cargo install --path crates/pq-cli
```

Or clone and use the Makefile:

```sh
git clone https://github.com/joewalnes/pq.git
cd pq
make install    # builds release binary, copies to ~/.local/bin/pq
```

## Features

**Inspection**
- `pq info` — file summary (size, rows, schema, compression, key-value metadata)
- `pq schema` — schema in multiple formats (tree, json-schema, arrow, DDL)
- `pq stats` — column statistics (min, max, null count, distinct count)
- `pq layout` — physical layout (row groups, column chunks, pages)

**Data access**
- `pq cat` — dump rows with `--limit`, `--offset`, `--columns`, `--where`, `--jq`
- `pq head` / `pq tail` — first or last N rows
- `pq sample` — random sample with optional `--seed` for reproducibility
- `pq count` — fast row count (reads metadata only, no full scan)
- `pq sql` — full SQL queries via [Apache DataFusion](https://datafusion.apache.org/)
- `pq jq` — jq expressions with `--slurp` and `--raw-output`
- `pq explore` — interactive TUI data explorer

**Transformation**
- `pq select` — project columns and filter rows into a new Parquet file
- `pq slice` — extract a row range into a new Parquet file
- `pq merge` — combine multiple files (strict, union, or intersect schema modes)
- `pq convert` — create Parquet from JSON, JSONL, or CSV (schema inferred automatically)

**Output modes** — every command supports all of these:
- `-O table` — pretty Unicode tables (default in a terminal)
- `-O jsonl` — one JSON object per line (default when piped)
- `-O json` — pretty-printed JSON array
- `-O csv` — RFC 4180 CSV
- `-O plain` — tab-separated values
- `pq capabilities` — machine-readable tool description for AI agents

## Building from source

Requires Rust 1.75+ and Cargo.

```sh
git clone https://github.com/joewalnes/pq.git
cd pq
make          # build + test + lint
make build    # cargo build --release
make test     # cargo test --workspace
make lint     # clippy + fmt check
make install  # install in ~/.local/bin/pq
```

## Author

Joe Walnes

## License

MIT OR Apache-2.0
