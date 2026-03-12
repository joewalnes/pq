# Getting Started

`pq` is a Swiss Army Knife for Parquet files. This tutorial walks through the
basics: importing data, inspecting files, and querying with SQL.

## Create a sample dataset

Start with a small JSON file containing user records:

**users.json**
```json
[
  {"name": "Alice", "age": 30, "city": "New York", "score": 92.5, "active": true},
  {"name": "Bob", "age": 25, "city": "Los Angeles", "score": 88.0, "active": true},
  {"name": "Charlie", "age": 35, "city": "Chicago", "score": 76.3, "active": false},
  {"name": "Diana", "age": 28, "city": "New York", "score": 95.1, "active": true},
  {"name": "Eve", "age": 32, "city": "Los Angeles", "score": 81.7, "active": false}
]
```

## Import into Parquet

Convert the JSON file into Parquet format:

```text
$ pq import users.json -o users.parquet
Converted 5 rows to users.parquet
```

## Inspect file metadata

Use `info` to see a summary of the file:

```text
$ pq info users.parquet
File:         users.parquet
Size:         1.8 KB
Rows:         5
Row Groups:   1
Columns:      5
Format:       v1.0
Created by:   parquet-rs version 53.4.1
Compression:  ZSTD(ZstdLevel(1))
Metadata:
  ARROW:schema:
    active: boolean (nullable)
    age: int64 (nullable)
    city: string (nullable)
    name: string (nullable)
    score: float64 (nullable)
```

## View the schema

The `schema` command shows column names and types:

```text
$ pq schema users.parquet
Schema (5 columns):
├── active: boolean (nullable)
├── age: int64 (nullable)
├── city: string (nullable)
├── name: string (nullable)
└── score: float64 (nullable)
```

## Preview data with head and tail

Show the first 3 rows:

```text
$ pq head users.parquet -n 3
╭────────┬─────┬─────────────┬─────────┬───────╮
│ active ┆ age ┆ city        ┆ name    ┆ score │
╞════════╪═════╪═════════════╪═════════╪═══════╡
│ true   ┆ 30  ┆ New York    ┆ Alice   ┆ 92.5  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ true   ┆ 25  ┆ Los Angeles ┆ Bob     ┆ 88.0  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ false  ┆ 35  ┆ Chicago     ┆ Charlie ┆ 76.3  │
╰────────┴─────┴─────────────┴─────────┴───────╯
```

Show the last 2 rows:

```text
$ pq tail users.parquet -n 2
╭────────┬─────┬─────────────┬───────┬───────╮
│ active ┆ age ┆ city        ┆ name  ┆ score │
╞════════╪═════╪═════════════╪═══════╪═══════╡
│ true   ┆ 28  ┆ New York    ┆ Diana ┆ 95.1  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ false  ┆ 32  ┆ Los Angeles ┆ Eve   ┆ 81.7  │
╰────────┴─────┴─────────────┴───────┴───────╯
```

## Count rows

```text
$ pq count users.parquet
5
```

## Dump rows with cat

Use `cat` with `--limit` to control how many rows are shown:

```text
$ pq cat users.parquet --limit 3
╭────────┬─────┬─────────────┬─────────┬───────╮
│ active ┆ age ┆ city        ┆ name    ┆ score │
╞════════╪═════╪═════════════╪═════════╪═══════╡
│ true   ┆ 30  ┆ New York    ┆ Alice   ┆ 92.5  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ true   ┆ 25  ┆ Los Angeles ┆ Bob     ┆ 88.0  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ false  ┆ 35  ┆ Chicago     ┆ Charlie ┆ 76.3  │
╰────────┴─────┴─────────────┴─────────┴───────╯
```

## Random sample

Use `--seed` for reproducible results:

```text
$ pq sample users.parquet -n 2 --seed 42
╭────────┬─────┬─────────────┬─────────┬───────╮
│ active ┆ age ┆ city        ┆ name    ┆ score │
╞════════╪═════╪═════════════╪═════════╪═══════╡
│ true   ┆ 25  ┆ Los Angeles ┆ Bob     ┆ 88.0  │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌┤
│ false  ┆ 35  ┆ Chicago     ┆ Charlie ┆ 76.3  │
╰────────┴─────┴─────────────┴─────────┴───────╯
```

## Query with SQL

Use the `sql` command to run SQL queries. Reference Parquet files in the FROM
clause with `./` paths:

```text
$ pq sql "SELECT name, age FROM './users.parquet' WHERE age > 28 ORDER BY age"
╭─────────┬─────╮
│ name    ┆ age │
╞═════════╪═════╡
│ Alice   ┆ 30  │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌┤
│ Eve     ┆ 32  │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌┤
│ Charlie ┆ 35  │
╰─────────┴─────╯
```

## Export back to JSON

Export the Parquet file back to JSON:

```text
$ pq export users.parquet -o users_out.json
Exported 5 rows to users_out.json
```

Verify the exported content:

```text
$ cat users_out.json
[
  {
    "active": true,
    "age": 30,
    "city": "New York",
    "name": "Alice",
    "score": 92.5
  },
  ...
]
```
