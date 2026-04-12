# Remote Files

pq can read Parquet files directly from URLs - no download needed. It lazily
fetches only the bytes it needs via HTTP range requests, so metadata commands
like `count`, `schema`, and `info` complete in milliseconds even on
multi-gigabyte files.

## Supported protocols

| Protocol | Example | Auth |
|----------|---------|------|
| HTTPS | `https://example.com/data.parquet` | None (public) |
| S3 | `s3://bucket/path.parquet` | AWS credentials from env |
| GCS | `gs://bucket/path.parquet` | GCP credentials from env |
| Azure | `az://container/path.parquet` | Azure credentials from env |

HTTPS works immediately for any public URL. Cloud storage protocols (S3,
GCS, Azure) read credentials from standard environment variables
(`AWS_ACCESS_KEY_ID`, `GOOGLE_APPLICATION_CREDENTIALS`, etc.).

## How range requests work

Parquet stores its metadata in the file footer. When pq opens a remote file,
it issues just 2-3 small HTTP requests:

1. **HEAD** - get the file size
2. **GET** (last 8 bytes) - read the footer length
3. **GET** (footer range) - read the schema, row counts, and column offsets

This means `pq count` on a 925 MB file transfers under 600 KB. Data commands
like `head` and `sql` then fetch only the row groups and columns they need,
never the entire file.

## Example: pq example data

Public example files are hosted at `data.pqtool.dev` for trying out pq
(see [Example Data](./example-data.md) for the full list). The 16 GB file
is perfect for demonstrating lazy loading:

```text
$ pq count "https://data.pqtool.dev/orders-100m.parquet"
100000000
```

That returns instantly - pq reads only the file footer (~600 bytes), not the
full 16 GB. You can run SQL against it too:

```sh
pq sql "SELECT shipping_address['city'] as city, count(*) n
         FROM 'https://data.pqtool.dev/orders-100k.parquet'
         GROUP BY shipping_address['city']
         ORDER BY n DESC
         LIMIT 5"
```

## Example: NYC Taxi Data (HTTPS)

The NYC Taxi & Limousine Commission publishes monthly trip data as public
Parquet files. Each file is ~50 MB with ~3 million rows.

Check the file metadata without downloading anything:

```text
$ pq schema "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet"
Schema (19 columns):
├── VendorID: int32 (nullable)
├── tpep_pickup_datetime: timestamp(us) (nullable)
├── tpep_dropoff_datetime: timestamp(us) (nullable)
├── passenger_count: int64 (nullable)
├── trip_distance: float64 (nullable)
├── RatecodeID: int64 (nullable)
├── store_and_fwd_flag: string (nullable)
├── PULocationID: int32 (nullable)
├── DOLocationID: int32 (nullable)
├── payment_type: int64 (nullable)
├── fare_amount: float64 (nullable)
├── extra: float64 (nullable)
├── mta_tax: float64 (nullable)
├── tip_amount: float64 (nullable)
├── tolls_amount: float64 (nullable)
├── improvement_surcharge: float64 (nullable)
├── total_amount: float64 (nullable)
├── congestion_surcharge: float64 (nullable)
└── Airport_fee: float64 (nullable)
```

Preview the first few rows (only fetches the first row group):

```text
$ pq head "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet" \
    -n 3 -c passenger_count,trip_distance,fare_amount,tip_amount,total_amount
╭─────────────────┬───────────────┬─────────────┬────────────┬──────────────╮
│ passenger_count ┆ trip_distance ┆ fare_amount ┆ tip_amount ┆ total_amount │
╞═════════════════╪═══════════════╪═════════════╪════════════╪══════════════╡
│ 1               ┆ 1.72          ┆ 17.7        ┆ 0.0        ┆ 22.7         │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1               ┆ 1.8           ┆ 10.0        ┆ 3.75       ┆ 18.75        │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ 1               ┆ 4.7           ┆ 23.3        ┆ 3.0        ┆ 31.3         │
╰─────────────────┴───────────────┴─────────────┴────────────┴──────────────╯
```

Run SQL directly against the remote file - DataFusion pushes predicates
down, so only matching row groups are fetched:

```text
$ pq sql "SELECT passenger_count,
                 ROUND(AVG(trip_distance), 2) as avg_distance,
                 ROUND(AVG(total_amount), 2) as avg_total,
                 COUNT(*) as trips
           FROM 'https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet'
           WHERE passenger_count IS NOT NULL
           GROUP BY passenger_count
           ORDER BY passenger_count
           LIMIT 6"
╭─────────────────┬──────────────┬───────────┬─────────╮
│ passenger_count ┆ avg_distance ┆ avg_total ┆ trips   │
╞═════════════════╪══════════════╪═══════════╪═════════╡
│ 0               ┆ 2.74         ┆ 25.33     ┆ 31465   │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ 1               ┆ 3.14         ┆ 26.21     ┆ 2188739 │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ 2               ┆ 3.78         ┆ 29.52     ┆ 405103  │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ 3               ┆ 3.66         ┆ 29.14     ┆ 91262   │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ 4               ┆ 3.88         ┆ 30.88     ┆ 51974   │
├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌┤
│ 5               ┆ 3.07         ┆ 26.27     ┆ 33506   │
╰─────────────────┴──────────────┴───────────┴─────────╯
```

## Debugging network requests

Pass `--debug` to see every HTTP request, including byte ranges:

```text
$ pq count "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet" --debug
[debug] HEAD trip-data/yellow_tripdata_2024-01.parquet  range=full  file_size=50.0 MB  ...
[debug] GET trip-data/yellow_tripdata_2024-01.parquet  range=Bounded(49961633..49961641)  file_size=50.0 MB  ...
[debug] GET trip-data/yellow_tripdata_2024-01.parquet  range=Bounded(49955276..49961633)  file_size=50.0 MB  ...
2964624
```

Three requests, all under 10 KB total. The entire 50 MB file is never downloaded.

## Cloud storage (S3, GCS, Azure)

For authenticated cloud storage, set credentials in your environment and use
native protocol URLs:

```sh
# AWS S3
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_DEFAULT_REGION=us-east-1
pq schema s3://my-bucket/data/events.parquet

# Google Cloud Storage
export GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
pq head gs://my-bucket/data/events.parquet -n 5

# Azure Blob Storage (native protocol)
export AZURE_STORAGE_ACCOUNT_NAME=...
export AZURE_STORAGE_ACCESS_KEY=...
pq sql "SELECT * FROM 'az://container/data.parquet' LIMIT 10"
```

All commands work identically whether the file is local or remote - the same
range-request optimization applies to S3, GCS, and Azure.
