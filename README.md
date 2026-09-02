# pq

A Parquet Swiss Army Knife. Inspect, query, transform, and view Parquet files from the command line.

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

# Peek at the data (pretty tables in a terminal, JSONL when piped)
$ pq head events.parquet
╭────┬───────┬─────────┬─────────────────────┬──────────┬──────────┬────────┬──────────╮
│ id ┆ event ┆ user_id ┆ ts                  ┆ page     ┆ duration ┆ active ┆ city     │
╞════╪═══════╪═════════╪═════════════════════╪══════════╪══════════╪════════╪══════════╡
│ 1  ┆ click ┆ 402     ┆ 2025-01-15T08:23:11 ┆ /home    ┆ 0.23     ┆ true   ┆ Seattle  │
├╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ 2  ┆ view  ┆ 117     ┆ 2025-01-15T08:23:14 ┆ /pricing ┆ 1.07     ┆ true   ┆ Portland │
╰────┴───────┴─────────┴─────────────────────┴──────────┴──────────┴────────┴──────────╯

# SQL queries - reference files directly in FROM
$ pq sql "SELECT city, count(*) n FROM 'events.parquet' WHERE active GROUP BY city ORDER BY n DESC LIMIT 3"
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

# Pipe-friendly: JSONL output when stdout isn't a terminal
$ pq cat events.parquet --columns city,event --where "duration > 1.0" | wc -l
23917

# Create parquet from JSON
$ pq import data.jsonl -o data.parquet

# Interactive TUI viewer (or just: pq events.parquet)
$ pq view events.parquet

# Works with remote files - lazily fetches only the bytes it needs
$ pq count "https://data.pqtool.dev/orders-100m.parquet"
100000000
```

## Try it now

No local files needed - public example data is hosted at `data.pqtool.dev`:

```sh
# Inspect a remote file (fetches only metadata)
pq schema "https://data.pqtool.dev/orders-10k.parquet"

# SQL query against a remote file
pq sql "SELECT status, count(*) n
         FROM 'https://data.pqtool.dev/orders-100k.parquet'
         GROUP BY status ORDER BY n DESC"

# Count 100 million rows without downloading the 16 GB file
pq count "https://data.pqtool.dev/orders-100m.parquet"
```

See [Example Data](https://pqtool.dev/example-data.html) for the full list of files and schema.

## Install

### Homebrew (macOS/Linux)

```sh
brew install joewalnes/tap/pq
```

### Download binary

```sh
# macOS (Apple Silicon)
curl -Lo pq https://github.com/joewalnes/pq/releases/latest/download/pq-darwin-arm64

# Linux (x86_64)
curl -Lo pq https://github.com/joewalnes/pq/releases/latest/download/pq-linux-amd64

# Linux (ARM)
curl -Lo pq https://github.com/joewalnes/pq/releases/latest/download/pq-linux-arm64
```

Then make it executable and move it to your PATH:

```sh
chmod +x pq
sudo mv pq /usr/local/bin/
```

### From source

Requires Rust 1.75+:

```sh
git clone https://github.com/joewalnes/pq.git
cd pq
make install    # builds release binary, copies to ~/.local/bin/pq
```

## Features

**Inspection**
- `pq info` - file summary (size, rows, schema, compression, key-value metadata)
- `pq schema` - schema in multiple formats (tree, json-schema, arrow, DDL)
- `pq stats` - column statistics (min, max, null count, distinct count)
- `pq layout` - physical layout (row groups, column chunks, pages)

**Data access**
- `pq cat` - dump rows with `--limit`, `--offset`, `--columns`, `--where`, `--jq`
- `pq head` / `pq tail` - first or last N rows
- `pq sample` - random sample with optional `--seed` for reproducibility
- `pq count` - fast row count (reads metadata only, no full scan)
- `pq grep` - search rows matching a regex across all (or selected) columns
- `pq sql` - full SQL queries via [Apache DataFusion](https://datafusion.apache.org/)
- `pq jq` - jq expressions with `--slurp` and `--raw-output`
- `pq view` - interactive TUI data viewer (default when a file is given without a subcommand)

**Transformation**
- `pq select` - project columns into a new Parquet file
- `pq slice` - extract a row range into a new Parquet file
- `pq merge` - combine multiple files (strict, union, or intersect schema modes)
- `pq split` - split a file by row count or partition column (Hive-style output)

**I/O**
- `pq import` - create Parquet from JSON, JSONL, or CSV (schema inferred automatically)
- `pq export` - export Parquet to CSV, JSON, or JSONL

**Validation**
- `pq validate` - check file integrity (footer, schema, statistics, data readability)

**Output modes** - the inspection, data-access, query, and `export` commands
above render via `-f`/`--format`:
- `-f table` - pretty Unicode tables (default in a terminal)
- `-f jsonl` - one JSON object per line (default when piped)
- `-f json` - pretty-printed JSON array
- `-f csv` - RFC 4180 CSV
- `-f plain` - tab-separated values

Commands that write a *file* instead of stdout pick the format from the
output file's extension by default. `-o` on `sql`/`export` honor an
explicit `-f` override (with a note on stderr) and refuse to guess when the
extension is unrecognized and `-f` isn't given either (see the
[FAQ](https://pqtool.dev/faq.html)). `-o` on `jq` and `-O` on `cat`
currently always use the extension and silently ignore `-f`
(tracked in `TODO.md`). The transformation commands (`select`, `slice`,
`merge`, `split`) and `import` always write an actual Parquet file and
ignore `-f`.

**Other**
- `pq capabilities` - machine-readable tool description for AI agents

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

### Integration tests

Remote-access tests (HTTP + S3) run against a local [SeaweedFS](https://github.com/seaweedfs/seaweedfs) container. Requires Docker.

```sh
make test-integration   # starts SeaweedFS, runs tests, tears down
```

You can also manage the container manually:

```sh
make test-seaweed-up    # start SeaweedFS container
cargo test --test remote_tests -- --ignored   # run tests
make test-seaweed-down  # stop container
```

### Documentation

```sh
make docs         # build site to docs/build/
make docs-serve   # build + start local server on :8000
```

The CLI reference page is auto-generated from `pq --help` output, so it
stays in sync with the code. The generator runs as part of `make docs`.

The tutorial *sources* in `tests/golden/tutorials/` double as integration
tests: `python3 tests/golden/run.py` executes each command block and
compares actual output against the expected output embedded in the doc, so
those files can't go stale. The *published* tutorials under
`docs/src/tutorials/` are a hand-formatted copy for the docs site (rendered
by `docs/build.py`'s plain Markdown pipeline, which can't parse the golden
runner's `console`/`file:` fences) and are **not** covered by that test -
they can and do drift from the tested originals. See `TODO.md`.

## Author

Joe Walnes

## License

MIT
