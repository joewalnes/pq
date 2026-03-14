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
<div class="feature-icon">&#x1F50D;</div>
<div class="feature-body">
<h3>Inspect</h3>
<p>File summary, schema, column statistics, physical layout, and validation</p>
<div class="cmds"><code>info</code> <code>schema</code> <code>stats</code> <code>layout</code> <code>validate</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4CA;</div>
<div class="feature-body">
<h3>Read</h3>
<p>Dump rows, preview head/tail, random sample, fast count, regex search</p>
<div class="cmds"><code>cat</code> <code>head</code> <code>tail</code> <code>sample</code> <code>count</code> <code>grep</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x26A1;</div>
<div class="feature-body">
<h3>Query</h3>
<p>Full SQL via Apache DataFusion and jq expressions via jaq</p>
<div class="cmds"><code>sql</code> <code>jq</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F527;</div>
<div class="feature-body">
<h3>Transform</h3>
<p>Project columns, extract row ranges, combine files, partition splits</p>
<div class="cmds"><code>select</code> <code>slice</code> <code>merge</code> <code>split</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4E6;</div>
<div class="feature-body">
<h3>Import &amp; Export</h3>
<p>Convert between Parquet, CSV, JSON, and JSONL</p>
<div class="cmds"><code>import</code> <code>export</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F5A5;</div>
<div class="feature-body">
<h3>Interactive Viewer</h3>
<p>Full-screen TUI with scrolling, column navigation, and remote file support</p>
<div class="cmds"><code>view</code></div>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F310;</div>
<div class="feature-body">
<h3>Remote Files</h3>
<p>HTTPS, S3, GCS, and Azure URLs work everywhere — only fetches the bytes it needs</p>
</div></div>
<div class="feature">
<div class="feature-icon">&#x1F4CB;</div>
<div class="feature-body">
<h3>Output Formats</h3>
<p>Pretty tables in a terminal, JSONL when piped, plus JSON, CSV, and plain TSV</p>
</div></div>
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
