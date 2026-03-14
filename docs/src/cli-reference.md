# CLI Reference

> **Auto-generated** - do not edit by hand.
> Run `./docs/generate-cli-reference.sh` to regenerate.

## pq

```text
A Parquet Swiss Army Knife - inspect, query, transform, and view Parquet files

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

## pq view

```text
Interactive TUI data viewer (default when a file is given without a subcommand)

Usage: pq view [OPTIONS] <FILE>

Arguments:
  <FILE>  Parquet file path

Options:
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq info

```text
Display file summary (size, rows, schema, compression, metadata)

Usage: pq info [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq schema

```text
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

## pq stats

```text
Display column statistics (min, max, nulls, distinct)

Usage: pq stats [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
      --describe                   Include data-level statistics (min, max, mean, stddev, distinct, top-K)
      --top <TOP>                  Number of top frequent values to show per column (with --describe) [default: 5]
      --sample-size <SAMPLE_SIZE>  Maximum rows to read for --describe (0 = all rows) [default: 100000]
  -f, --format <OUTPUT_FORMAT>     Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>              Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                      Suppress non-essential output
  -v, --verbose                    Increase verbosity
  -h, --help                       Print help
```

## pq layout

```text
Display physical layout (row groups, column chunks, pages)

Usage: pq layout [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq validate

```text
Validate Parquet file integrity (footer, schema, statistics)

Usage: pq validate [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq cat

```text
Dump rows from a Parquet file

Usage: pq cat [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -l, --limit <LIMIT>           Maximum number of rows to output
  -o, --offset <OFFSET>         Number of rows to skip
  -c, --columns <COLUMNS>       Columns to include (comma-separated)
  -w, --where <WHERE_CLAUSE>    SQL WHERE clause to filter rows
      --jq <JQ>                 jq expression to apply to each row
  -O, --output <OUTPUT>         Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq head

```text
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

## pq tail

```text
Show last N rows (default 10)

Usage: pq tail [OPTIONS] <FILES>...

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

## pq sample

```text
Show random N rows

Usage: pq sample [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -n, --lines <LINES>           Number of rows to sample [default: 10]
      --seed <SEED>             Random seed for reproducibility
  -c, --columns <COLUMNS>       Columns to include (comma-separated)
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq count

```text
Fast row count (metadata-only when possible)

Usage: pq count [OPTIONS] [FILES]...

Arguments:
  [FILES]...  Parquet file paths

Options:
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq grep

```text
Search rows matching a regex pattern across all columns.

By default, searches all string-representable columns. Use -c to restrict
to specific columns. Returns matching rows in the output format.

Examples:
  pq grep data.parquet 'error|warn'           # regex across all columns
  pq grep data.parquet 'alice' -i             # case-insensitive
  pq grep data.parquet '404' -c status,code   # search specific columns
  pq grep data.parquet 'timeout' --limit 10   # first 10 matches

Usage: pq grep [OPTIONS] <FILES>... <PATTERN>

Arguments:
  <FILES>...
          Parquet file path(s)

  <PATTERN>
          Regex pattern to search for

Options:
  -c, --columns <COLUMNS>
          Columns to search (comma-separated; default: all)

  -l, --limit <LIMIT>
          Maximum number of matching rows to return

  -i, --ignore-case
          Case-insensitive matching

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

## pq sql

```text
Execute SQL queries on Parquet files using Apache DataFusion.

Files are referenced directly in the FROM clause using single-quoted paths.
Glob patterns (e.g., 'logs/*.parquet') are supported.

Examples:
  pq sql "SELECT * FROM 'data.parquet' LIMIT 10"
  pq sql "SELECT city, count(*) FROM 'data.parquet' GROUP BY city"
  pq sql "SELECT a.id, b.name FROM 'a.parquet' a JOIN 'b.parquet' b ON a.id = b.id"
  pq sql "SELECT * FROM 'logs/*.parquet' WHERE level = 'ERROR'"

SQL reference: https://datafusion.apache.org/user-guide/sql/index.html

Usage: pq sql [OPTIONS] [QUERY]

Arguments:
  [QUERY]
          SQL query (files can be referenced directly in FROM clause)

Options:
  -o, --output <OUTPUT>
          Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)

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

## pq jq

```text
Apply jq expressions to Parquet data.

Each row is processed as a JSON object. Use --slurp to collect all rows
into an array first.

Examples:
  pq jq data.parquet '.name'                           # extract field
  pq jq data.parquet '{name, age}' -r                  # construct objects
  pq jq data.parquet 'select(.age > 30)'               # filter rows
  pq jq data.parquet '[.orders[].price] | add'         # nested aggregation
  pq jq data.parquet 'group_by(.city) | map({city: .[0].city, n: length})' -s

jq reference: https://jqlang.github.io/jq/manual/

Usage: pq jq [OPTIONS] <FILES>... <FILTER>

Arguments:
  <FILES>...
          Parquet file path(s)

  <FILTER>
          jq filter expression

Options:
  -s, --slurp
          Read all rows into an array before filtering

  -r, --raw-output
          Output raw strings without JSON quoting

  -o, --output <OUTPUT>
          Write output to a file (format auto-detected from extension: .parquet, .json, .jsonl, .csv)

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

## pq select

```text
Project columns into a new Parquet file.

Examples:
  pq select data.parquet -c id,name -o subset.parquet
  pq select data.parquet -c 'id,name,address' -o subset.parquet

Usage: pq select [OPTIONS] --columns <COLUMNS> --output <OUTPUT> <FILE>

Arguments:
  <FILE>
          Input Parquet file

Options:
  -c, --columns <COLUMNS>
          Columns to include (comma-separated)

  -o, --output <OUTPUT>
          Output file path

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

## pq slice

```text
Extract a row range into a new Parquet file

Usage: pq slice [OPTIONS] --limit <LIMIT> --output <OUTPUT> <FILE>

Arguments:
  <FILE>  Input Parquet file

Options:
      --offset <OFFSET>         Start offset [default: 0]
      --limit <LIMIT>           Number of rows to extract
  -o, --output <OUTPUT>         Output file path
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq merge

```text
Combine multiple Parquet files into one

Usage: pq merge [OPTIONS] --output <OUTPUT> [FILES]...

Arguments:
  [FILES]...  Input Parquet files

Options:
  -o, --output <OUTPUT>            Output file path
      --schema-mode <SCHEMA_MODE>  Schema reconciliation mode [default: strict] [possible values: strict, union, intersect]
  -f, --format <OUTPUT_FORMAT>     Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>              Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                      Suppress non-essential output
  -v, --verbose                    Increase verbosity
  -h, --help                       Print help
```

## pq split

```text
Split a Parquet file into multiple files

Usage: pq split [OPTIONS] --output <OUTPUT> <FILE>

Arguments:
  <FILE>  Input Parquet file

Options:
      --rows <ROWS>                  Number of rows per output file
      --partition-by <PARTITION_BY>  Column(s) to partition by (comma-separated, Hive-style output)
  -o, --output <OUTPUT>              Output directory
  -f, --format <OUTPUT_FORMAT>       Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>                Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                        Suppress non-essential output
  -v, --verbose                      Increase verbosity
  -h, --help                         Print help
```

## pq import

```text
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

## pq export

```text
Export Parquet data to CSV, JSON, or JSONL

Usage: pq export [OPTIONS] <FILES>...

Arguments:
  <FILES>...  Parquet file path(s)

Options:
  -o, --output <OUTPUT>         Output file path (default: stdout)
  -l, --limit <LIMIT>           Maximum number of rows to export
  -f, --format <OUTPUT_FORMAT>  Output format (table, json, jsonl, csv, plain) [possible values: json, jsonl, csv, table, plain]
      --color <COLOR>           Color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Suppress non-essential output
  -v, --verbose                 Increase verbosity
  -h, --help                    Print help
```

## pq completions

```text
Generate shell completions

Add to your ~/.zshrc (zsh):
  eval "$(pq completions zsh)"

Add to your ~/.bashrc (bash):
  eval "$(pq completions bash)"

Add to your ~/.config/fish/config.fish (fish):
  pq completions fish | source

Usage: pq completions [OPTIONS] <SHELL>

Arguments:
  <SHELL>
          Shell to generate completions for
          
          [possible values: bash, elvish, fish, powershell, zsh]

Options:
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

