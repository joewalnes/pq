# Help Output

Snapshot tests for help text. These catch regressions when command-line
arguments or descriptions change.

## Main help

```console
$ pq --help
A Parquet Swiss Army Knife — inspect, query, transform, and view Parquet files

Usage: pq [OPTIONS] <COMMAND>

Viewer:
  view          Interactive TUI data viewer (default)

Metadata:
  info          Display file summary (size, rows, schema, compression)
  schema        Display schema (tree, json-schema, arrow, ddl, pyarrow)
  stats         Display column statistics (min, max, nulls, distinct)
  layout        Display physical layout (row groups, pages)
  validate      Validate file integrity

Data:
  cat           Dump rows
  head          Show first N rows
  tail          Show last N rows
  sample        Show random N rows
  count         Fast row count
  grep          Search rows by regex

Query:
  sql           Execute SQL via DataFusion
  jq            Apply jq expressions

Transform:
  select        Project columns
  slice         Extract row range
  merge         Combine files
  split         Split file

I/O:
  import        Import CSV/JSON/JSONL to Parquet
  export        Export Parquet to CSV/JSON/JSONL

  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
  -V, --version                 Print version



Examples:
  pq data.parquet                              # open in TUI viewer
  pq info data.parquet
  pq cat data.parquet --limit 100
  pq sql "SELECT count(*) FROM 'data.parquet'"
  pq jq data.parquet '.name'
```

## Schema subcommand help

```console
$ pq schema --help
Display schema in various formats.

Styles:
  tree        Indented tree (default)
  json        JSON object
  json-schema JSON Schema
  arrow       Arrow type names
  ddl         PostgreSQL-compatible CREATE TABLE
  pyarrow     Python PyArrow schema constructor

Usage: pq schema [OPTIONS] <FILES>...

Arguments:
  <FILES>...
          Parquet file path(s)

Options:
  -s, --style <STYLE>
          Schema style

          Possible values:
          - tree
          - json
          - json-schema
          - arrow
          - ddl:         PostgreSQL-compatible DDL (CREATE TABLE)
          - pyarrow

          [default: tree]

  -f, --format <OUTPUT_FORMAT>
          Output format (table, json, jsonl, csv, plain)

          [possible values: json, jsonl, csv, table, plain]

      --color <COLOR>
          Color output

          [default: auto]
          [possible values: auto, always, never]

  -q, --quiet
          Suppress non-essential output

  -v, --verbose
          Increase verbosity

  -h, --help
          Print help (see a summary with '-h')
```

## SQL subcommand help

Running `pq sql` with no query shows the long help:

```console
$ pq sql
Execute SQL queries on Parquet files using Apache DataFusion.

Files are referenced directly in the FROM clause using single-quoted paths.
Glob patterns (e.g., 'logs/*.parquet') are supported.

Examples:
  pq sql "SELECT * FROM 'data.parquet' LIMIT 10"
  pq sql "SELECT city, count(*) FROM 'data.parquet' GROUP BY city"
  pq sql "SELECT a.id, b.name FROM 'a.parquet' a JOIN 'b.parquet' b ON a.id = b.id"
  pq sql "SELECT * FROM 'logs/*.parquet' WHERE level = 'ERROR'"

SQL reference: https://datafusion.apache.org/user-guide/sql/index.html

Usage: sql [QUERY]

Arguments:
  [QUERY]
          SQL query (files can be referenced directly in FROM clause)

Options:
  -h, --help
          Print help (see a summary with '-h')
```

## Head subcommand help

```console
$ pq head --help
Show first N rows (default 10)

Usage: pq head [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -n, --lines <LINES>           Number of rows to show [default: 10]
  -c, --columns <COLUMNS>       Columns to include (comma-separated)
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## Import subcommand help

```console
$ pq import --help
Import CSV/JSON/JSONL into Parquet format

Usage: pq import [OPTIONS] --output <OUTPUT> <INPUT>

Arguments:
  <INPUT>  Input file (JSON, JSONL, or CSV)

Options:
  -o, --output <OUTPUT>              Output Parquet file path
  -F, --input-format <INPUT_FORMAT>  Input format (auto-detected from extension if not specified) [possible values: json, jsonl, csv]
  -f, --format <OUTPUT_FORMAT>       Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>                Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                        Suppress non-essential output
  -v, --verbose                      Increase verbosity
  -h, --help                         Print help
```
