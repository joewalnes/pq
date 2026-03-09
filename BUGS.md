# Bugs & Tasks

## P1

### ~~Remote file access (S3, HTTP)~~ FIXED

All commands currently require a local filesystem path (`std::fs::File` in `pq-core/src/reader.rs`).
Users should be able to pass a URL instead:

```sh
pq head s3://my-bucket/events.parquet
pq sql "SELECT * FROM 'https://example.com/data.parquet' LIMIT 10"
pq info https://example.com/data.parquet
```

Expected support:
- **HTTP/HTTPS** — using range requests (`Range` header) so only the footer + requested row groups are fetched, not the entire file.
- **S3** (`s3://`) — via the `object_store` crate, which DataFusion already uses under the hood.
- **GCS** (`gs://`) and **Azure** (`az://`, `abfss://`) — same crate, lower priority.

This affects `pq-core/src/reader.rs` (and DataFusion session setup in `pq sql`) since both need to resolve a path-or-URL into a reader. The `object_store` + `parquet::arrow::async_reader` crates already support async range-request reads for all of these backends.

**Resolution**: Added `object_store` (HTTP + S3) integration. All read-only commands (`info`, `schema`, `stats`, `layout`, `cat`, `head`, `tail`, `sample`, `count`, `sql`, `jq`, `explore`) now accept `https://` and `s3://` URLs. Uses `ParquetObjectReader` for async range-request reads — only the footer and requested row groups are fetched. Transform commands (`select`, `slice`, `merge`, `convert`) remain local-only. GCS and Azure URL schemes are detected but not yet wired (requires adding `gcp`/`azure` features to the `object_store` dependency).
