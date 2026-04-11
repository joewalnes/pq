# TODO

## Tier 1 — High impact

- [x] Multi-file path support — All data commands now accept multiple files and glob patterns (e.g. `pq cat data/*.parquet`). Glob expansion via `files::resolve_files()`.
- [ ] `diff <a> <b>` — Compare two parquet files: schema diff (added/removed/changed columns, type changes) and optional data diff (row-level, sampled or full)
- [ ] `repack <file>` — Rewrite with different compression (`--compression zstd|snappy|gzip|none`), row group size (`--row-group-size`), encoding, or sort order
- [ ] `sort <file>` — Sort by one or more columns (`--by col1,col2 --desc`) and write new file; could also be a flag on `repack`
- [x] `export <file>` — Parquet to CSV/JSON/JSONL file output (`-o output.csv`); format auto-detected from extension
- [x] Remote file access — S3 and HTTPS supported for all read-only commands via `object_store` + `ParquetObjectReader`. Only footer + requested row groups are fetched. Transform commands remain local-only.
- [x] Remote file access: GCS (`gs://`) and Azure (`az://`, `abfss://`) — wired via `gcp`/`azure` features on `object_store`; credentials read from environment

## Tier 2 — Differentiation

- [x] `describe <file>` — Statistical summary per column: count, nulls, null%, min, max, mean, stddev, distinct, top-K frequent values
- [x] `grep <file> <pattern>` — Search across all columns for a regex match; supports `-i` case-insensitive, `--limit`, `-c` column filter
- [x] `split <file>` — Split by row count (`--rows`) or partition key (`--partition-by col`); Hive-style partitioned output
- [ ] `schema evolve <file>` — Add columns (`--add name:type`), drop (`--drop col`), rename (`--rename old:new`), cast types (`--cast col:type`)
- [x] `validate <file>` — Check file integrity: valid footer, row count consistency, column count per row group, statistics sanity, data readability

## Bugs

- [x] TUI: row number unreadable on selected row — Fixed: row number now uses `fg(White)` on selected row, `fg(DarkGray)` otherwise.
- [ ] P2: Interactive viewer demo GIF missing from docs site — The homepage and viewer page reference `img/tui-viewer.gif` but the GIF has never been committed. `make demos` generates it via `demos/tui-viewer.py` + asciinema + agg, but the docs CI workflow doesn't run this step. Either generate and commit the GIF, or add asciinema/agg to the docs workflow.

## Infrastructure

- [x] Streaming TUI — Lazy PageCache with background fetch thread; no row limit
- [ ] `union <files...>` — Like merge but actually implements union-by-name (fill missing columns with nulls)
- [ ] Progress bars on all transform commands (repack, sort, merge, convert, export, split)
- [x] Fix `merge --schema-mode union|intersect` — implemented: union adds null columns for missing fields, intersect keeps only common columns, strict rejects mismatches
