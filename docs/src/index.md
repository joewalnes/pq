# pq

A Parquet Swiss Army Knife. Inspect, query, transform, and view Parquet files from the command line.

```sh
# What's in this file?
$ pq info events.parquet
File:         events.parquet
Size:         1.2 GiB
Rows:         48,291,037
Row Groups:   12
Columns:      8
Compression:  ZSTD

# Peek at the data
$ pq head events.parquet -n 3
╭────┬───────┬─────────┬──────────────────────┬──────────╮
│ id ┆ event ┆ user_id ┆ ts                   ┆ city     │
╞════╪═══════╪═════════╪══════════════════════╪══════════╡
│  1 ┆ click ┆     402 ┆ 2025-01-15T08:23:11Z ┆ Seattle  │
├╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│  2 ┆ view  ┆     117 ┆ 2025-01-15T08:23:14Z ┆ Portland │
├╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│  3 ┆ click ┆     892 ┆ 2025-01-15T08:23:19Z ┆ Denver   │
╰────┴───────┴─────────┴──────────────────────┴──────────╯

# SQL queries — reference files directly in FROM
$ pq sql "SELECT city, count(*) n FROM 'events.parquet' GROUP BY city ORDER BY n DESC LIMIT 3"
╭──────────┬───────╮
│ city     ┆ n     │
╞══════════╪═══════╡
│ Seattle  ┆ 12485 │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ Portland ┆ 9712  │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ Denver   ┆ 8033  │
╰──────────┴───────╯

# jq expressions
$ pq jq events.parquet '{city, event}' | head -2
{"city":"Seattle","event":"click"}
{"city":"Portland","event":"view"}

# Works with remote files too — only fetches the bytes it needs
$ pq count "https://example.com/big-dataset.parquet"
2964624
```

## Features

**Inspection** — `info`, `schema`, `stats`, `layout`, `validate`

**Data access** — `cat`, `head`, `tail`, `sample`, `count`, `grep`

**Query** — `sql` (via Apache DataFusion), `jq` (via jaq)

**Transform** — `select`, `slice`, `merge`, `split`

**I/O** — `import` CSV/JSON/JSONL to Parquet, `export` back out

**Interactive viewer** — TUI data viewer with scrolling and column navigation

**Remote files** — HTTPS, S3, GCS, and Azure URLs work everywhere. HTTP range requests mean metadata commands complete in milliseconds even on multi-gigabyte files.

**Output formats** — Pretty tables in a terminal, JSONL when piped. Also: `json`, `csv`, `plain`. Override with `-f`.

## Install

```sh
cargo install --path crates/pq-cli
```

Or build from source:

```sh
git clone git@git.corp.stripe.com:joejoejoe/pq.git
cd pq
make install    # builds release binary, copies to ~/.local/bin/pq
```

## What's next

- [Interactive Viewer](./viewer.md) — navigate data with the TUI
- [Getting Started tutorial](./tutorials/getting-started.md) — import, inspect, query, export
- [CLI Reference](./cli-reference.md) — every command and flag
