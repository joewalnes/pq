# Example Data

Public Parquet files hosted at `data.pqtool.dev` for trying out pq. No
download needed - pq reads remote files lazily, fetching only the bytes it
needs.

## Available files

| File | Rows | Size | Description |
|------|------|------|-------------|
| [orders-10k.parquet](https://data.pqtool.dev/orders-10k.parquet) | 10,000 | 2 MB | Tiny - instant for any command |
| [orders-100k.parquet](https://data.pqtool.dev/orders-100k.parquet) | 100,000 | 19 MB | Small - good for SQL and jq tutorials |
| [orders-100m.parquet](https://data.pqtool.dev/orders-100m.parquet) | 100,000,000 | 16 GB | Large - demonstrates lazy loading and range requests |

All three files share the same schema: a synthetic e-commerce orders dataset
with 30 columns covering a wide range of Parquet types.

## Quick start

```sh
# Inspect the schema (fetches only metadata, ~1 KB)
pq schema "https://data.pqtool.dev/orders-10k.parquet"

# Preview rows
pq head "https://data.pqtool.dev/orders-10k.parquet"

# SQL query
pq sql "SELECT status, count(*) n
         FROM 'https://data.pqtool.dev/orders-100k.parquet'
         GROUP BY status ORDER BY n DESC"

# Count 100 million rows (reads only the footer, ~600 bytes)
pq count "https://data.pqtool.dev/orders-100m.parquet"
```

## Schema

```text
Schema (30 columns):
├── order_id: int64 (nullable)
├── user_id: int32 (nullable)
├── order_date: date (nullable)
├── created_at: timestamp(us) (nullable)
├── updated_at: timestamp(us) (nullable)
├── status: string (nullable)
├── is_priority: boolean (nullable)
├── is_returning_customer: boolean (nullable)
├── subtotal: float64 (nullable)
├── tax_amount: float32 (nullable)
├── discount_pct: float64 (nullable)
├── total_amount: float64 (nullable)
├── currency: string (nullable)
├── item_count: int16 (nullable)
├── customer_name: string (nullable)
├── email: string (nullable)
├── age: int8 (nullable)
├── loyalty_points: uint32 (nullable)
├── weight_kg: decimal(10,3) (nullable)
├── session_token: fixed_binary(16) (nullable)
├── shipping_address: struct (nullable)
│   ├── street: string (nullable)
│   ├── city: string (nullable)
│   ├── state: string (nullable)
│   ├── zip: string (nullable)
│   └── country: string (nullable)
├── billing_address: struct (nullable)
│   ├── street: string (nullable)
│   ├── city: string (nullable)
│   ├── state: string (nullable)
│   ├── zip: string (nullable)
│   └── country: string (nullable)
├── tags: list<string> (nullable)
├── ratings: list<int8> (nullable)
├── line_items: list<struct> (nullable)
│   ├── product_id: int32 (nullable)
│   ├── product_name: string (nullable)
│   ├── quantity: int16 (nullable)
│   ├── unit_price: float64 (nullable)
│   └── category: string (nullable)
├── payment: struct (nullable)
│   ├── method: string (nullable)
│   ├── card_last_four: string (nullable)
│   └── processor_response: struct (nullable)
│       ├── code: string (nullable)
│       ├── message: string (nullable)
│       └── risk_score: float32 (nullable)
├── metadata: map (nullable)
│   ├── key: string
│   └── value: string (nullable)
├── notes: string (nullable)
├── referral_source: string (nullable)
└── session_duration_ms: int64 (nullable)
```

Verified against `pq schema orders-10k.parquet -f table` (`generate_test_parquet.py`
marks every top-level and nested field nullable, regardless of its actual null
rate — the Arrow schema itself doesn't encode "5%-80%", only the data does).

The dataset includes primitives (int, float, bool, string), temporal types
(date, timestamp), nested structs (including 3-level nesting via
`payment.processor_response`), lists of scalars and structs, a map, a
decimal, fixed-size binary, and columns with varying null rates (5%-80%) —
the null *rate* varies per column, but the schema marks every column
nullable.

## Download locally

If you want a local copy:

```sh
curl -Lo orders-10k.parquet https://data.pqtool.dev/orders-10k.parquet
curl -Lo orders-100k.parquet https://data.pqtool.dev/orders-100k.parquet
```

The 16 GB file is best used remotely via pq's lazy loading - there's no need
to download it.
