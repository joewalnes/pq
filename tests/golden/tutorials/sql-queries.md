# SQL Queries with pq

`pq sql` executes SQL queries on Parquet files using Apache DataFusion.
Files are referenced in the FROM clause with `./` relative paths.

## Setup: create test data

A users table:

```file:users.json
[
  {"name": "Alice", "age": 30, "city": "New York", "score": 92.5, "active": true},
  {"name": "Bob", "age": 25, "city": "Los Angeles", "score": 88.0, "active": true},
  {"name": "Charlie", "age": 35, "city": "Chicago", "score": 76.3, "active": false},
  {"name": "Diana", "age": 28, "city": "New York", "score": 95.1, "active": true},
  {"name": "Eve", "age": 32, "city": "Los Angeles", "score": 81.7, "active": false}
]
```

An orders table:

```file:orders.json
[
  {"order_id": 1, "customer": "Alice", "product": "Widget", "quantity": 3, "price": 9.99},
  {"order_id": 2, "customer": "Bob", "product": "Gadget", "quantity": 1, "price": 24.99},
  {"order_id": 3, "customer": "Alice", "product": "Gadget", "quantity": 2, "price": 24.99},
  {"order_id": 4, "customer": "Charlie", "product": "Widget", "quantity": 5, "price": 9.99},
  {"order_id": 5, "customer": "Diana", "product": "Doohickey", "quantity": 1, "price": 49.99},
  {"order_id": 6, "customer": "Bob", "product": "Widget", "quantity": 2, "price": 9.99},
  {"order_id": 7, "customer": "Eve", "product": "Gadget", "quantity": 1, "price": 24.99},
  {"order_id": 8, "customer": "Alice", "product": "Doohickey", "quantity": 1, "price": 49.99}
]
```

Import both files:

```console
$ pq import users.json -o users.parquet
Converted 5 rows to users.parquet
$ pq import orders.json -o orders.parquet
Converted 8 rows to orders.parquet
```

## SELECT with WHERE and ORDER BY

Filter and sort rows:

```console
$ pq sql "SELECT customer, product, quantity FROM './orders.parquet' WHERE quantity > 1 ORDER BY quantity DESC" -f table
╭──────────┬─────────┬──────────╮
│ customer ┆ product ┆ quantity │
╞══════════╪═════════╪══════════╡
│ Charlie  ┆ Widget  ┆ 5        │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ Alice    ┆ Widget  ┆ 3        │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ Alice    ┆ Gadget  ┆ 2        │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┤
│ Bob      ┆ Widget  ┆ 2        │
╰──────────┴─────────┴──────────╯
```

## LIMIT

Return only the top results:

```console
$ pq sql "SELECT product, SUM(quantity) as total_qty FROM './orders.parquet' GROUP BY product ORDER BY total_qty DESC LIMIT 2" -f table
╭─────────┬───────────╮
│ product ┆ total_qty │
╞═════════╪═══════════╡
│ Widget  ┆ 10        │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┤
│ Gadget  ┆ 4         │
╰─────────┴───────────╯
```

## GROUP BY with aggregates

Summarize orders per customer:

```console
$ pq sql "SELECT customer, COUNT(*) as num_orders, SUM(quantity * price) as total_spent FROM './orders.parquet' GROUP BY customer ORDER BY total_spent DESC" -f table
╭──────────┬────────────┬─────────────╮
│ customer ┆ num_orders ┆ total_spent │
╞══════════╪════════════╪═════════════╡
│ Alice    ┆ 3          ┆ 129.94      │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Diana    ┆ 1          ┆ 49.99       │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Charlie  ┆ 1          ┆ 49.95       │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Bob      ┆ 2          ┆ 44.97       │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Eve      ┆ 1          ┆ 24.99       │
╰──────────┴────────────┴─────────────╯
```

## Aggregate functions

Compute min, max, average, and sum across all orders:

```console
$ pq sql "SELECT MIN(price) as min_price, MAX(price) as max_price, ROUND(AVG(price), 2) as avg_price, SUM(quantity) as total_items FROM './orders.parquet'" -f table
╭───────────┬───────────┬───────────┬─────────────╮
│ min_price ┆ max_price ┆ avg_price ┆ total_items │
╞═══════════╪═══════════╪═══════════╪═════════════╡
│ 9.99      ┆ 49.99     ┆ 25.62     ┆ 16          │
╰───────────┴───────────┴───────────┴─────────────╯
```

## JOIN two files

Join users with their orders to see order counts by city:

```console
$ pq sql "SELECT u.name, u.city, COUNT(o.order_id) as num_orders FROM './users.parquet' u JOIN './orders.parquet' o ON u.name = o.customer GROUP BY u.name, u.city ORDER BY num_orders DESC" -f table
╭─────────┬─────────────┬────────────╮
│ name    ┆ city        ┆ num_orders │
╞═════════╪═════════════╪════════════╡
│ Alice   ┆ New York    ┆ 3          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Bob     ┆ Los Angeles ┆ 2          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Diana   ┆ New York    ┆ 1          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Eve     ┆ Los Angeles ┆ 1          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Charlie ┆ Chicago     ┆ 1          │
╰─────────┴─────────────┴────────────╯
```
