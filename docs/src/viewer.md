# Interactive Viewer

`pq view` opens a full-screen TUI for browsing Parquet data. It's also the default when you pass a file without a subcommand:

```sh
pq data.parquet          # opens the viewer
pq view data.parquet     # same thing
```

![TUI viewer demo](img/tui-viewer.gif)

## Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` / `j` / `k` | Scroll rows |
| `←` / `→` / `h` / `l` | Scroll columns |
| `Page Up` / `Page Down` | Jump by page |
| `Home` / `End` | Jump to first / last row |
| `q` / `Esc` | Quit |

## When to use it

The viewer is useful when you want to explore a file interactively: scroll through rows, check column values, and get a feel for the data. For scripted or piped workflows, use `cat`, `head`, or `sql` instead.

## Remote files

The viewer works with remote URLs too. pq fetches row groups on demand as you scroll:

```sh
pq "https://example.com/data.parquet"
pq "s3://my-bucket/data.parquet"
```

Try it with a real dataset — this [Overture Maps](https://overturemaps.org/) places file is ~200 MB, but pq only fetches the bytes it needs:

```sh
pq "https://overturemapswestus2.blob.core.windows.net/release/2026-02-18.0/theme=places/type=place/part-00000-308cb36d-c529-4dc2-83bb-fe6b282a2b1a-c000.zstd.parquet"
```
