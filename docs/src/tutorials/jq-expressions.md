# jq Expressions

pq includes built-in jq support (via the [jaq](https://github.com/01mf02/jaq) library) for transforming and
filtering Parquet data. This works with both the `pq jq` command and the
`pq cat --jq` flag.

## Setup

Create an employee dataset with nested fields:

**employees.json**
```json
[
  {"name": "Alice", "department": "Engineering", "salary": 120000, "skills": ["rust", "python", "sql"], "address": {"city": "New York", "state": "NY"}},
  {"name": "Bob", "department": "Engineering", "salary": 105000, "skills": ["go", "python"], "address": {"city": "San Francisco", "state": "CA"}},
  {"name": "Charlie", "department": "Marketing", "salary": 95000, "skills": ["analytics", "sql"], "address": {"city": "Chicago", "state": "IL"}},
  {"name": "Diana", "department": "Engineering", "salary": 130000, "skills": ["rust", "c++", "python"], "address": {"city": "Seattle", "state": "WA"}},
  {"name": "Eve", "department": "Marketing", "salary": 98000, "skills": ["design", "analytics"], "address": {"city": "New York", "state": "NY"}},
  {"name": "Frank", "department": "Sales", "salary": 110000, "skills": ["negotiation", "analytics"], "address": {"city": "Los Angeles", "state": "CA"}},
  {"name": "Grace", "department": "Engineering", "salary": 115000, "skills": ["python", "sql", "rust"], "address": {"city": "New York", "state": "NY"}},
  {"name": "Hank", "department": "Sales", "salary": 102000, "skills": ["negotiation", "sql"], "address": {"city": "Chicago", "state": "IL"}}
]
```

```text
$ pq import employees.json -o employees.parquet
Converted 8 rows to employees.parquet
```

## Extracting fields

Use `-r` (raw output) to get plain strings without JSON quoting:

```text
$ pq jq employees.parquet '.name' -r
Alice
Bob
Charlie
Diana
Eve
Frank
Grace
Hank
```

## Accessing nested fields

Dot notation reaches into structs:

```text
$ pq jq employees.parquet '.address.city' -r
New York
San Francisco
Chicago
Seattle
New York
Los Angeles
New York
Chicago
```

## Building new objects

Construct objects with the fields you need:

```text
$ pq jq employees.parquet '{name: .name, dept: .department}'
{"dept":"Engineering","name":"Alice"}
{"dept":"Engineering","name":"Bob"}
{"dept":"Marketing","name":"Charlie"}
{"dept":"Engineering","name":"Diana"}
{"dept":"Marketing","name":"Eve"}
{"dept":"Sales","name":"Frank"}
{"dept":"Engineering","name":"Grace"}
{"dept":"Sales","name":"Hank"}
```

## Filtering with select

`select()` keeps rows that match a condition:

```text
$ pq jq employees.parquet 'select(.salary > 110000) | {name, salary}'
{"name":"Alice","salary":120000}
{"name":"Diana","salary":130000}
{"name":"Grace","salary":115000}
```

## Working with arrays

Count items in an array field:

```text
$ pq jq employees.parquet '{name: .name, num_skills: (.skills | length)}'
{"name":"Alice","num_skills":3}
{"name":"Bob","num_skills":2}
{"name":"Charlie","num_skills":2}
{"name":"Diana","num_skills":3}
{"name":"Eve","num_skills":2}
{"name":"Frank","num_skills":2}
{"name":"Grace","num_skills":3}
{"name":"Hank","num_skills":2}
```

Explode arrays: `.skills[]` emits one output per element:

```text
$ pq jq employees.parquet '.skills[]' -r
rust
python
sql
go
python
analytics
sql
...
```

## Filtering by array contents

Find employees who know Rust:

```text
$ pq jq employees.parquet 'select(.skills | contains(["rust"])) | .name' -r
Alice
Diana
Grace
```

## Conditional logic

Use `if/elif/else/end` to classify rows:

```text
$ pq jq employees.parquet '{name: .name, level: (if .salary >= 115000 then "senior" elif .salary >= 100000 then "mid" else "junior" end)}'
{"level":"senior","name":"Alice"}
{"level":"mid","name":"Bob"}
{"level":"junior","name":"Charlie"}
{"level":"senior","name":"Diana"}
{"level":"junior","name":"Eve"}
{"level":"mid","name":"Frank"}
{"level":"senior","name":"Grace"}
{"level":"mid","name":"Hank"}
```

## String interpolation

Build formatted strings with `\()`:

```text
$ pq jq employees.parquet '"\(.name) works in \(.department) (\(.address.city))"' -r
Alice works in Engineering (New York)
Bob works in Engineering (San Francisco)
Charlie works in Marketing (Chicago)
Diana works in Engineering (Seattle)
Eve works in Marketing (New York)
Frank works in Sales (Los Angeles)
Grace works in Engineering (New York)
Hank works in Sales (Chicago)
```

## Slurp mode

By default, jq processes one row at a time. `--slurp` collects all rows
into a single array first, enabling aggregations.

Count total rows:

```text
$ pq jq employees.parquet 'length' --slurp
8
```

Group by department and compute averages:

```text
$ pq jq employees.parquet \
    'group_by(.department) | map({
       department: .[0].department,
       count: length,
       avg_salary: (map(.salary) | add / length)
     })' --slurp
[{"avg_salary":117500.0,"count":4,"department":"Engineering"},
 {"avg_salary":96500.0,"count":2,"department":"Marketing"},
 {"avg_salary":106000.0,"count":2,"department":"Sales"}]
```

Find top 3 earners:

```text
$ pq jq employees.parquet 'sort_by(.salary) | reverse | .[:3] | .[].name' --slurp -r
Diana
Alice
Grace
```

List unique cities:

```text
$ pq jq employees.parquet '[.[].address.city] | unique | .[]' --slurp -r
Chicago
Los Angeles
New York
San Francisco
Seattle
```

## Saving results to files

Use `-o` to write jq results to a file. Format is auto-detected from the extension.

Save transformed data to Parquet:

```text
$ pq jq employees.parquet '{name, city: .address.city}' -o flat.parquet
Wrote 8 rows to flat.parquet

$ pq cat flat.parquet
╭───────────────┬─────────╮
│ city          ┆ name    │
╞═══════════════╪═════════╡
│ New York      ┆ Alice   │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ San Francisco ┆ Bob     │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ Chicago       ┆ Charlie │
│ ...           ┆ ...     │
╰───────────────┴─────────╯
```

## Using --jq with cat

The `--jq` flag on `pq cat` lets you combine row limiting with jq transforms:

```text
$ pq cat employees.parquet --jq '{name, city: .address.city}' --limit 3
{"city":"New York","name":"Alice"}
{"city":"San Francisco","name":"Bob"}
{"city":"Chicago","name":"Charlie"}
```

Save filtered data with `cat --jq -O`:

```text
$ pq cat employees.parquet --jq 'select(.salary > 110000) | {name, salary}' \
    -O high_earners.parquet
Wrote 3 rows to high_earners.parquet
```

## jq reference

pq uses [jaq](https://github.com/01mf02/jaq), a Rust implementation of jq.
See the [jq manual](https://jqlang.github.io/jq/manual/) for the full language reference.
Most jq features are supported.
