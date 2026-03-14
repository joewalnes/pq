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

# What columns does it have?
$ pq schema events.parquet
Schema (8 columns):
├── id: int64
├── event: string
├── user_id: int32
├── ts: timestamp(us)
├── city: string
├── device: string
├── duration_ms: int32
╰── payload: struct
    ├── action: string
    ╰── metadata: map<string, string>

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

# SQL queries - reference files directly in FROM
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

# Works with remote files too - lazily fetches only the bytes it needs
$ pq count "https://example.com/big-dataset.parquet"
2964624
```

## Features

<div class="features">
<div class="feature">
<div class="feature-icon">&#x1F50D;</div>
<div class="feature-body">
<h3>Inspect</h3>
<p>File summary, schema, column statistics, physical layout, and validation</p>
<div class="cmds"><a href="cli-reference.html#pq-info"><code>info</code></a> <a href="cli-reference.html#pq-schema"><code>schema</code></a> <a href="cli-reference.html#pq-stats"><code>stats</code></a> <a href="cli-reference.html#pq-layout"><code>layout</code></a> <a href="cli-reference.html#pq-validate"><code>validate</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4CA;</div>
<div class="feature-body">
<h3>Read</h3>
<p>Dump rows, preview head/tail, random sample, fast count, regex search</p>
<div class="cmds"><a href="cli-reference.html#pq-cat"><code>cat</code></a> <a href="cli-reference.html#pq-head"><code>head</code></a> <a href="cli-reference.html#pq-tail"><code>tail</code></a> <a href="cli-reference.html#pq-sample"><code>sample</code></a> <a href="cli-reference.html#pq-count"><code>count</code></a> <a href="cli-reference.html#pq-grep"><code>grep</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x26A1;</div>
<div class="feature-body">
<h3>Query</h3>
<p>Full SQL via Apache DataFusion and jq expressions via jaq</p>
<div class="cmds"><a href="cli-reference.html#pq-sql"><code>sql</code></a> <a href="cli-reference.html#pq-jq"><code>jq</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F527;</div>
<div class="feature-body">
<h3>Transform</h3>
<p>Project columns, extract row ranges, combine files, partition splits</p>
<div class="cmds"><a href="cli-reference.html#pq-select"><code>select</code></a> <a href="cli-reference.html#pq-slice"><code>slice</code></a> <a href="cli-reference.html#pq-merge"><code>merge</code></a> <a href="cli-reference.html#pq-split"><code>split</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4E6;</div>
<div class="feature-body">
<h3>Import &amp; Export</h3>
<p>Convert between Parquet, CSV, JSON, and JSONL</p>
<div class="cmds"><a href="cli-reference.html#pq-import"><code>import</code></a> <a href="cli-reference.html#pq-export"><code>export</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F5A5;</div>
<div class="feature-body">
<h3>Interactive Viewer</h3>
<p>Full-screen TUI with scrolling, column navigation, and remote file support</p>
<div class="cmds"><a href="viewer.html"><code>view</code></a></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F310;</div>
<div class="feature-body">
<h3>Remote Files</h3>
<p>HTTPS, S3, GCS, and Azure URLs work everywhere, lazily fetching only the bytes it needs</p>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4CB;</div>
<div class="feature-body">
<h3>Output Formats</h3>
<p>Pretty tables in a terminal, JSONL when piped, plus JSON, CSV, and plain TSV</p>
</div></div>
</div>

## Interactive viewer

![TUI viewer demo](img/tui-viewer.gif)

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

## Getting started

- [Interactive Viewer](./viewer.md) - navigate data with the TUI
- [Getting Started tutorial](./tutorials/getting-started.md) - import, inspect, query, export
- [CLI Reference](./cli-reference.md) - every command and flag
