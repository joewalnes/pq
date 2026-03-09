# TODO

## Tier 1 — High impact

- [ ] Multi-file path support — Allow all data commands (`cat`, `head`, `tail`, `sample`, `sql`, `jq`, `explore`, `stats`, etc.) to accept multiple files or glob patterns, treating them as one logical dataset split across parts. Currently only `count` and `merge` accept multiple files; every other command takes a single `file: String` in `cli.rs`.
- [ ] `diff <a> <b>` — Compare two parquet files: schema diff (added/removed/changed columns, type changes) and optional data diff (row-level, sampled or full)
- [ ] `repack <file>` — Rewrite with different compression (`--compression zstd|snappy|gzip|none`), row group size (`--row-group-size`), encoding, or sort order
- [ ] `sort <file>` — Sort by one or more columns (`--by col1,col2 --desc`) and write new file; could also be a flag on `repack`
- [ ] `export <file>` — Parquet to CSV/JSON/JSONL file output (`-o output.csv`); proper streaming export with progress for large files
- [ ] Remote file access — Support `s3://`, `gs://`, `https://` URLs anywhere a file path is accepted via `object_store` crate

## Tier 2 — Differentiation

- [ ] `describe <file>` — Statistical summary per column: mean, median, stddev, min, max, null%, cardinality, top-K frequent values
- [ ] `grep <file> <pattern>` — Search across all (or specified) columns for a regex/literal match, return matching rows
- [ ] `split <file>` — Split by row count (`--rows`), file size (`--size`), or partition key (`--partition-by col`); Hive-style partitioned output
- [ ] `schema evolve <file>` — Add columns (`--add name:type`), drop (`--drop col`), rename (`--rename old:new`), cast types (`--cast col:type`)
- [ ] `validate <file>` — Check file integrity: valid footer, page checksums, schema consistency across row groups, statistics sanity

## Infrastructure

- [ ] Streaming TUI — Remove 10K row cap in `explore`; lazy-load batches as user scrolls
- [ ] `union <files...>` — Like merge but actually implements union-by-name (fill missing columns with nulls)
- [ ] Progress bars on all transform commands (repack, sort, merge, convert, export, split)
- [ ] Fix `merge --schema-mode union|intersect` — currently both just use first file's schema
