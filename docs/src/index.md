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

<div class="features">
<div class="feature">
<h3>Inspection</h3>
<code>info</code> <code>schema</code> <code>stats</code> <code>layout</code> <code>validate</code>
</div>
<div class="feature">
<h3>Data Access</h3>
<code>cat</code> <code>head</code> <code>tail</code> <code>sample</code> <code>count</code> <code>grep</code>
</div>
<div class="feature">
<h3>Query</h3>
<code>sql</code> via Apache DataFusion<br>
<code>jq</code> via jaq
</div>
<div class="feature">
<h3>Transform</h3>
<code>select</code> <code>slice</code> <code>merge</code> <code>split</code>
</div>
<div class="feature">
<h3>I/O</h3>
<code>import</code> CSV/JSON/JSONL to Parquet<br>
<code>export</code> Parquet back out
</div>
<div class="feature">
<h3>Interactive Viewer</h3>
TUI data viewer with scrolling and column navigation
</div>
<div class="feature">
<h3>Remote Files</h3>
HTTPS, S3, GCS, and Azure URLs work everywhere — only fetches the bytes it needs
</div>
<div class="feature">
<h3>Output Formats</h3>
Pretty tables in a terminal, JSONL when piped. Also: <code>json</code> <code>csv</code> <code>plain</code>
</div>
</div>

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
