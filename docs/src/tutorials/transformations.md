# Transformations

pq includes commands for restructuring Parquet files: selecting columns,
slicing rows, merging files, and splitting into partitions.

## Setup

Create a product catalog:

**products.json**
```json
[
  {"id": 1, "name": "Widget A", "category": "tools", "price": 9.99, "stock": 150},
  {"id": 2, "name": "Widget B", "category": "tools", "price": 14.99, "stock": 80},
  {"id": 3, "name": "Gadget X", "category": "electronics", "price": 49.99, "stock": 30},
  {"id": 4, "name": "Gadget Y", "category": "electronics", "price": 79.99, "stock": 15},
  {"id": 5, "name": "Gizmo", "category": "electronics", "price": 199.99, "stock": 5},
  {"id": 6, "name": "Thingamajig", "category": "misc", "price": 4.99, "stock": 500},
  {"id": 7, "name": "Doohickey", "category": "tools", "price": 24.99, "stock": 60},
  {"id": 8, "name": "Whatchamacallit", "category": "misc", "price": 2.99, "stock": 1000}
]
```

```text
$ pq import products.json -o products.parquet
Converted 8 rows to products.parquet
```

## Select: project columns

`pq select` creates a new file with only the columns you need:

```text
$ pq select products.parquet -c name,price -o name_price.parquet
Wrote 8 rows to name_price.parquet
```

The output file has a reduced schema:

```text
$ pq schema name_price.parquet
Schema (2 columns):
├── name: string (nullable)
└── price: float64 (nullable)
```

```text
$ pq head name_price.parquet
╭─────────────────┬────────╮
│ name            ┆ price  │
╞═════════════════╪════════╡
│ Widget A        ┆ 9.99   │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┤
│ Widget B        ┆ 14.99  │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┤
│ Gadget X        ┆ 49.99  │
│ ...             ┆ ...    │
╰─────────────────┴────────╯
```

## Slice: extract a row range

`pq slice` extracts a contiguous range of rows:

```text
$ pq slice products.parquet --offset 2 --limit 3 -o middle.parquet
Wrote 3 rows to middle.parquet

$ pq cat middle.parquet
╭─────────────┬────┬──────────┬────────┬───────╮
│ category    ┆ id ┆ name     ┆ price  ┆ stock │
╞═════════════╪════╪══════════╪════════╪═══════╡
│ electronics ┆ 3  ┆ Gadget X ┆ 49.99  ┆ 30    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ electronics ┆ 4  ┆ Gadget Y ┆ 79.99  ┆ 15    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ electronics ┆ 5  ┆ Gizmo    ┆ 199.99 ┆ 5     │
╰─────────────┴────┴──────────┴────────┴───────╯
```

## Merge: combine files

Create a second file with new products:

**products_new.json**
```json
[
  {"id": 9, "name": "Sprocket", "category": "tools", "price": 34.99, "stock": 40},
  {"id": 10, "name": "Doodad", "category": "misc", "price": 7.99, "stock": 200}
]
```

```text
$ pq import products_new.json -o products_new.parquet
Converted 2 rows to products_new.parquet
```

`pq merge` concatenates files with matching schemas:

```text
$ pq merge products.parquet products_new.parquet -o all_products.parquet
Merged 2 files, wrote 10 rows to all_products.parquet

$ pq count all_products.parquet
10
```

### Schema modes

By default, `merge` uses strict mode — all files must have identical schemas.
Use `--schema-mode` to handle schema differences:

| Mode | Behavior |
|------|----------|
| `strict` | All schemas must match exactly (default) |
| `union` | Keep all columns, fill missing with nulls |
| `intersect` | Keep only columns present in all files |

## Split by row count

`pq split --rows N` breaks a file into chunks of N rows each:

```text
$ pq split products.parquet --rows 3 -o split_rows/
Wrote 3 rows to split_rows/products_0000.parquet
Wrote 3 rows to split_rows/products_0001.parquet
Wrote 2 rows to split_rows/products_0002.parquet
Split 8 rows into 3 files in split_rows/
```

## Split by partition column

`--partition-by` creates Hive-style directory partitions, useful for tools
like Spark, Trino, or DuckDB:

```text
$ pq split products.parquet --partition-by category -o split_category/
```

Each partition directory contains only the matching rows:

```text
$ pq cat split_category/category=electronics/products.parquet
╭─────────────┬────┬──────────┬────────┬───────╮
│ category    ┆ id ┆ name     ┆ price  ┆ stock │
╞═════════════╪════╪══════════╪════════╪═══════╡
│ electronics ┆ 3  ┆ Gadget X ┆ 49.99  ┆ 30    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ electronics ┆ 4  ┆ Gadget Y ┆ 79.99  ┆ 15    │
├╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ electronics ┆ 5  ┆ Gizmo    ┆ 199.99 ┆ 5     │
╰─────────────┴────┴──────────┴────────┴───────╯

$ pq cat split_category/category=tools/products.parquet
╭──────────┬────┬───────────┬───────┬───────╮
│ category ┆ id ┆ name      ┆ price ┆ stock │
╞══════════╪════╪═══════════╪═══════╪═══════╡
│ tools    ┆ 1  ┆ Widget A  ┆ 9.99  ┆ 150   │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ tools    ┆ 2  ┆ Widget B  ┆ 14.99 ┆ 80    │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ tools    ┆ 7  ┆ Doohickey ┆ 24.99 ┆ 60    │
╰──────────┴────┴───────────┴───────┴───────╯
```
