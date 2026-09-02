# SQL Queries

`pq sql` executes SQL queries on Parquet files using Apache DataFusion.
Files are referenced in the FROM clause with single-quoted paths.

## Setup: create test data

A users table:

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

An orders table:

**orders.json**
```json
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

```text
$ pq import users.json -o users.parquet
Converted 5 rows to users.parquet
$ pq import orders.json -o orders.parquet
Converted 8 rows to orders.parquet
```

## SELECT with WHERE and ORDER BY

Filter and sort rows:

```text
$ pq sql "SELECT customer, product, quantity
           FROM './orders.parquet'
           WHERE quantity > 1
           ORDER BY quantity DESC"
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

```text
$ pq sql "SELECT product, SUM(quantity) as total_qty
           FROM './orders.parquet'
           GROUP BY product
           ORDER BY total_qty DESC
           LIMIT 2"
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

```text
$ pq sql "SELECT customer,
                 COUNT(*) as num_orders,
                 SUM(quantity * price) as total_spent
           FROM './orders.parquet'
           GROUP BY customer
           ORDER BY total_spent DESC"
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

```text
$ pq sql "SELECT MIN(price) as min_price,
                 MAX(price) as max_price,
                 ROUND(AVG(price), 2) as avg_price,
                 SUM(quantity) as total_items
           FROM './orders.parquet'"
╭───────────┬───────────┬───────────┬─────────────╮
│ min_price ┆ max_price ┆ avg_price ┆ total_items │
╞═══════════╪═══════════╪═══════════╪═════════════╡
│ 9.99      ┆ 49.99     ┆ 25.62     ┆ 16          │
╰───────────┴───────────┴───────────┴─────────────╯
```

## Saving results to files

Use `-o` to write query results to a file. The format is auto-detected from
the extension.

Save to Parquet:

```text
$ pq sql "SELECT name, age FROM './users.parquet' WHERE active = true ORDER BY age" \
    -o active_users.parquet
Wrote 3 rows to active_users.parquet

$ pq cat active_users.parquet
╭───────┬─────╮
│ name  ┆ age │
╞═══════╪═════╡
│ Bob   ┆ 25  │
├╌╌╌╌╌╌╌┼╌╌╌╌╌┤
│ Diana ┆ 28  │
├╌╌╌╌╌╌╌┼╌╌╌╌╌┤
│ Alice ┆ 30  │
╰───────┴─────╯
```

Save to JSON:

```text
$ pq sql "SELECT name, score FROM './users.parquet' ORDER BY score DESC LIMIT 3" \
    -o top_scores.json
Wrote 3 rows to top_scores.json

$ cat top_scores.json
[
  {"name": "Diana", "score": 95.1},
  {"name": "Alice", "score": 92.5},
  {"name": "Bob", "score": 88.0}
]
```

## JOIN two files

Join users with their orders to see order counts by city:

```text
$ pq sql "SELECT u.name, u.city, COUNT(o.order_id) as num_orders
           FROM './users.parquet' u
           JOIN './orders.parquet' o ON u.name = o.customer
           GROUP BY u.name, u.city
           ORDER BY num_orders DESC, u.name"
╭─────────┬─────────────┬────────────╮
│ name    ┆ city        ┆ num_orders │
╞═════════╪═════════════╪════════════╡
│ Alice   ┆ New York    ┆ 3          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Bob     ┆ Los Angeles ┆ 2          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Charlie ┆ Chicago     ┆ 1          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Diana   ┆ New York    ┆ 1          │
├╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Eve     ┆ Los Angeles ┆ 1          │
╰─────────┴─────────────┴────────────╯
```

## Querying nested fields

Parquet files often contain structs, lists, and maps. DataFusion supports
bracket notation to reach into nested columns.

Create a file with nested data:

**events.json**
```json
[
  {"event": "click", "user_id": 402, "city": "Seattle", "payload": {"action": "buy", "metadata": {"source": "ad", "campaign": "summer"}}},
  {"event": "view", "user_id": 117, "city": "Portland", "payload": {"action": "browse", "metadata": {"source": "organic", "campaign": null}}},
  {"event": "click", "user_id": 892, "city": "Denver", "payload": {"action": "buy", "metadata": {"source": "ad", "campaign": "winter"}}},
  {"event": "view", "user_id": 402, "city": "Seattle", "payload": {"action": "browse", "metadata": {"source": "referral", "campaign": null}}},
  {"event": "click", "user_id": 117, "city": "Portland", "payload": {"action": "signup", "metadata": {"source": "ad", "campaign": "summer"}}}
]
```

```text
$ pq import events.json -o events.parquet
Converted 5 rows to events.parquet
```

Access struct fields with bracket notation:

```text
$ pq sql "SELECT event, payload['action'] as action, city
           FROM './events.parquet'
           WHERE payload['action'] = 'buy'"
╭───────┬────────┬─────────╮
│ event ┆ action ┆ city    │
╞═══════╪════════╪═════════╡
│ click ┆ buy    ┆ Seattle │
├╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ click ┆ buy    ┆ Denver  │
╰───────┴────────┴─────────╯
```

Reach into deeply nested structs by chaining brackets:

```text
$ pq sql "SELECT payload['action'] as action,
                 payload['metadata']['source'] as source,
                 COUNT(*) as n
           FROM './events.parquet'
           GROUP BY payload['action'], payload['metadata']['source']
           ORDER BY n DESC"
╭────────┬──────────┬───╮
│ action ┆ source   ┆ n │
╞════════╪══════════╪═══╡
│ buy    ┆ ad       ┆ 2 │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌┤
│ browse ┆ organic  ┆ 1 │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌┤
│ browse ┆ referral ┆ 1 │
├╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌┼╌╌╌┤
│ signup ┆ ad       ┆ 1 │
╰────────┴──────────┴───╯
```

Filter on nested values and join with flat data:

```text
$ pq sql "SELECT e.city, COUNT(*) as ad_clicks
           FROM './events.parquet' e
           WHERE e.event = 'click'
             AND e.payload['metadata']['source'] = 'ad'
           GROUP BY e.city
           ORDER BY ad_clicks DESC"
╭──────────┬────────────╮
│ city     ┆ ad_clicks  │
╞══════════╪════════════╡
│ Portland ┆ 1          │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Denver   ┆ 1          │
├╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┤
│ Seattle  ┆ 1          │
╰──────────┴────────────╯
```

## SQL reference

pq uses [Apache DataFusion](https://datafusion.apache.org/) for SQL execution.
See the [DataFusion SQL reference](https://datafusion.apache.org/user-guide/sql/index.html)
for the full list of supported functions and syntax.
