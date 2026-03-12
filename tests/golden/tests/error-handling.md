# Error Handling

Tests that error messages are clear and exit codes are correct.

## Missing file

```console
$ pq info nonexistent.parquet  # [exit: 1]
Error: Failed to open file 'nonexistent.parquet': No such file or directory (os error 2): No such file or directory (os error 2)
```

## Invalid format flag

```console
$ pq cat users.parquet -f badformat  # [exit: 2]
error: invalid value 'badformat' for '--format <OUTPUT_FORMAT>'
  [possible values: json, jsonl, csv, table, plain]

For more information, try '--help'.
```

## Missing required arguments

Running `import` without any arguments shows usage help:

```console
$ pq import  # [exit: 2]
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

## Invalid SQL query

```console
$ pq sql "SELECT * FROM"  # [exit: 1]
Error: DataFusion error: SQL error: ParserError("Expected: identifier, found: EOF"): SQL error: ParserError("Expected: identifier, found: EOF"): sql parser error: Expected: identifier, found: EOF
```

## Schema on missing file

```console
$ pq schema missing.parquet  # [exit: 1]
Error: Failed to open file 'missing.parquet': No such file or directory (os error 2): No such file or directory (os error 2)
```

## Head on missing file

```console
$ pq head missing.parquet  # [exit: 1]
Error: Failed to open file 'missing.parquet': No such file or directory (os error 2): No such file or directory (os error 2)
```

## Count on missing file

```console
$ pq count missing.parquet  # [exit: 1]
Error: Failed to open file 'missing.parquet': No such file or directory (os error 2): No such file or directory (os error 2)
```
