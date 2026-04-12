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
├── order_id: int64
├── user_id: int32
├── order_date: date32
├── created_at: timestamp(us)
├── updated_at: timestamp(us) (nullable)
├── status: string
├── is_priority: boolean
├── is_returning_customer: boolean
├── subtotal: float64
├── tax_amount: float32
├── discount_pct: float64
├── total_amount: float64
├── currency: string
├── item_count: int16
├── customer_name: string
├── email: string
├── age: int8 (nullable)
├── loyalty_points: uint32 (nullable)
├── weight_kg: decimal128(10,3)
├── session_token: binary(16)
├── shipping_address: struct
│   ├── street: string
│   ├── city: string
│   ├── state: string
│   ├── zip: string
│   └── country: string
├── billing_address: struct (nullable)
│   ├── street: string
│   ├── city: string
│   ├── state: string
│   ├── zip: string
│   └── country: string
├── tags: list<string> (nullable)
├── ratings: list<int8> (nullable)
├── line_items: list<struct>
│   ├── product_id: int32
│   ├── product_name: string
│   ├── quantity: int16
│   ├── unit_price: float64
│   └── category: string
├── payment: struct
│   ├── method: string
│   ├── card_last_four: string (nullable)
│   └── processor_response: struct
│       ├── code: string
│       ├── message: string
│       └── risk_score: float32
├── metadata: map<string, string>
├── notes: string (nullable)
├── referral_source: string (nullable)
└── session_duration_ms: int64
```

The dataset includes primitives (int, float, bool, string), temporal types
(date, timestamp), nested structs (including 3-level nesting via
`payment.processor_response`), lists of scalars and structs, maps, decimals,
fixed-size binary, and columns with varying null rates (5%-80%).

## Download locally

If you want a local copy:

```sh
curl -Lo orders-10k.parquet https://data.pqtool.dev/orders-10k.parquet
curl -Lo orders-100k.parquet https://data.pqtool.dev/orders-100k.parquet
```

The 16 GB file is best used remotely via pq's lazy loading - there's no need
to download it.
