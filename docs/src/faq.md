# FAQ

## What output format does pq use?

By default, pq uses pretty Unicode tables when stdout is a terminal, and JSONL
(one JSON object per line) when piped. You can override this with `-f`:

```sh
pq head data.parquet -f json     # pretty JSON array
pq head data.parquet -f jsonl    # one JSON object per line
pq head data.parquet -f csv      # RFC 4180 CSV
pq head data.parquet -f table    # Unicode table (default in terminal)
pq head data.parquet -f plain    # tab-separated values
```

## How do I reference files in SQL queries?

Use single-quoted paths in the FROM clause. Prefix local files with `./`:

```sh
pq sql "SELECT * FROM './data.parquet' LIMIT 10"
pq sql "SELECT * FROM './logs/*.parquet' WHERE level = 'ERROR'"
pq sql "SELECT * FROM 'https://example.com/data.parquet' LIMIT 10"
```

Glob patterns are supported for querying multiple files at once.

## What SQL dialect does pq use?

pq uses [Apache DataFusion](https://datafusion.apache.org/) for SQL execution.
See the [DataFusion SQL reference](https://datafusion.apache.org/user-guide/sql/index.html)
for the full list of supported functions, types, and syntax.

## What jq features are supported?

pq uses [jaq](https://github.com/01mf02/jaq), a Rust implementation of jq.
Most jq features are supported, including: object construction, `select()`,
`map()`, `group_by()`, `sort_by()`, array slicing, string interpolation,
`if/elif/else/end`, `try/catch`, and `--slurp` mode.

See the [jq manual](https://jqlang.github.io/jq/manual/) for the language reference.

## Does pq download entire remote files?

No. pq uses HTTP range requests to fetch only the bytes it needs. Metadata
commands (`count`, `schema`, `info`) typically transfer under 1 KB even for
multi-gigabyte files. Data commands fetch only the row groups and columns
required. Use `--debug` to see the exact HTTP requests.

## What cloud storage is supported?

- **S3** - `s3://bucket/path.parquet` (uses `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_DEFAULT_REGION`)
- **GCS** - `gs://bucket/path.parquet` (uses `GOOGLE_APPLICATION_CREDENTIALS`)
- **Azure** - `az://container/path.parquet` (uses `AZURE_STORAGE_ACCOUNT_NAME`, `AZURE_STORAGE_ACCESS_KEY`)
- **HTTPS** - any public URL, no auth needed

## How do I generate shell completions?

```sh
# zsh - add to ~/.zshrc
eval "$(pq completions zsh)"

# bash - add to ~/.bashrc
eval "$(pq completions bash)"

# fish - add to ~/.config/fish/config.fish
pq completions fish | source
```

## What Parquet versions and compression codecs are supported?

pq reads Parquet v1 and v2 files with any standard compression codec
(Snappy, ZSTD, GZIP, LZ4, Brotli, uncompressed). It writes ZSTD-compressed
Parquet v1 files by default.

## How do I convert CSV or JSON to Parquet?

```sh
pq import data.csv -o data.parquet
pq import data.json -o data.parquet
pq import data.jsonl -o data.parquet
```

The input format is auto-detected from the file extension. Use `-F` to
override: `pq import data.txt -F csv -o data.parquet`.

## How do I convert Parquet back to CSV or JSON?

```sh
pq export data.parquet -o data.csv
pq export data.parquet -o data.json
pq export data.parquet -o data.jsonl
```

The output format is auto-detected from the extension.
