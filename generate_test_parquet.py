#!/usr/bin/env python3
"""Generate a realistic ~10M row Parquet file for testing.

Produces an e-commerce orders dataset with 30 columns demonstrating:
- Primitives: int8/16/32/64, uint32, float32/64, bool, string
- Temporal: date32, timestamp[us]
- Nested: struct, nested struct, list<string>, list<int8>, list<struct>
- Other: decimal128, fixed_size_binary, map<string,string>
- Nullable columns with varying null rates (5%-80%)

Requirements: pip install pyarrow numpy
Usage: python generate_test_parquet.py [-o OUTPUT] [-n ROWS] [--seed SEED]
"""

import argparse
import time
from datetime import date
from decimal import Decimal

import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq

# ── Word lists ───────────────────────────────────────────────────────────────

FIRST_NAMES = [
    "James", "Mary", "Robert", "Patricia", "John", "Jennifer", "Michael",
    "Linda", "David", "Elizabeth", "William", "Barbara", "Richard", "Susan",
    "Joseph", "Jessica", "Thomas", "Sarah", "Christopher", "Karen", "Charles",
    "Lisa", "Daniel", "Nancy", "Matthew", "Betty", "Anthony", "Margaret",
    "Mark", "Sandra", "Donald", "Ashley", "Steven", "Kimberly", "Paul",
    "Emily", "Andrew", "Donna", "Joshua", "Michelle", "Kenneth", "Carol",
    "Kevin", "Amanda", "Brian", "Dorothy", "George", "Melissa", "Timothy",
    "Deborah", "Ronald", "Stephanie", "Edward", "Rebecca", "Jason", "Sharon",
    "Jeffrey", "Laura", "Ryan", "Cynthia", "Jacob", "Kathleen", "Gary",
    "Amy", "Nicholas", "Angela", "Eric", "Shirley", "Jonathan", "Anna",
    "Stephen", "Brenda", "Larry", "Pamela", "Justin", "Emma", "Scott",
    "Nicole", "Brandon", "Helen", "Benjamin", "Samantha", "Samuel",
    "Katherine", "Raymond", "Christine", "Gregory", "Debra", "Frank",
    "Rachel", "Alexander", "Carolyn", "Patrick", "Janet", "Jack", "Catherine",
    "Dennis", "Heather", "Jerry", "Teresa", "Tyler", "Diane",
]

LAST_NAMES = [
    "Smith", "Johnson", "Williams", "Brown", "Jones", "Garcia", "Miller",
    "Davis", "Rodriguez", "Martinez", "Hernandez", "Lopez", "Gonzalez",
    "Wilson", "Anderson", "Thomas", "Taylor", "Moore", "Jackson", "Martin",
    "Lee", "Perez", "Thompson", "White", "Harris", "Sanchez", "Clark",
    "Ramirez", "Lewis", "Robinson", "Walker", "Young", "Allen", "King",
    "Wright", "Scott", "Torres", "Nguyen", "Hill", "Flores", "Green",
    "Adams", "Nelson", "Baker", "Hall", "Rivera", "Campbell", "Mitchell",
    "Carter", "Roberts", "Gomez", "Phillips", "Evans", "Turner", "Diaz",
    "Parker", "Cruz", "Edwards", "Collins", "Reyes", "Stewart", "Morris",
    "Morales", "Murphy", "Cook", "Rogers", "Gutierrez", "Ortiz", "Morgan",
    "Cooper", "Peterson", "Bailey", "Reed", "Kelly", "Howard", "Ramos",
    "Kim", "Cox", "Ward", "Richardson", "Watson", "Brooks", "Chavez",
    "Wood", "James", "Bennett", "Gray", "Mendoza", "Ruiz", "Hughes",
    "Price", "Alvarez", "Castillo", "Sanders", "Patel", "Myers", "Long",
    "Ross", "Foster",
]

STREET_NAMES = [
    "Main", "Oak", "Cedar", "Maple", "Elm", "Pine", "Washington", "Lake",
    "Hill", "Walnut", "Spring", "Park", "Sunset", "Ridge", "Meadow",
    "River", "Forest", "Valley", "Highland", "Willow", "Franklin",
    "Lincoln", "Jefferson", "Madison",
]

STREET_TYPES = ["St", "Ave", "Blvd", "Dr", "Ln", "Way", "Ct", "Rd", "Pl"]

CITIES = [
    "New York", "Los Angeles", "Chicago", "Houston", "Phoenix",
    "Philadelphia", "San Antonio", "San Diego", "Dallas", "San Jose",
    "Austin", "Jacksonville", "Fort Worth", "Columbus", "Charlotte",
    "Indianapolis", "San Francisco", "Seattle", "Denver", "Nashville",
    "Oklahoma City", "El Paso", "Portland", "Las Vegas", "Memphis",
    "Louisville", "Baltimore", "Milwaukee", "Albuquerque", "Tucson",
    "Fresno", "Sacramento", "Mesa", "Kansas City", "Atlanta", "Omaha",
    "Colorado Springs", "Raleigh", "Long Beach", "Virginia Beach",
    "Miami", "Oakland", "Minneapolis", "Tampa", "Tulsa", "Arlington",
    "New Orleans", "Wichita",
]

STATES = [
    "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "FL", "GA", "HI",
    "ID", "IL", "IN", "IA", "KS", "KY", "LA", "ME", "MD", "MA", "MI",
    "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ", "NM", "NY", "NC",
    "ND", "OH", "OK", "OR", "PA", "RI", "SC", "SD", "TN", "TX", "UT",
    "VT", "VA", "WA", "WV", "WI", "WY",
]

# Weighted toward US
COUNTRIES = ["US", "US", "US", "US", "US", "US", "US", "US", "CA", "GB"]

PRODUCT_ADJECTIVES = [
    "Premium", "Classic", "Ultra", "Essential", "Professional", "Deluxe",
    "Organic", "Natural", "Advanced", "Basic", "Vintage", "Modern",
    "Compact", "Heavy-Duty", "Lightweight", "Portable", "Wireless", "Smart",
]

PRODUCT_NOUNS = [
    "Widget", "Gadget", "Laptop", "Headphones", "Speaker", "Camera",
    "Keyboard", "Mouse", "Monitor", "Charger", "Cable", "Adapter",
    "Backpack", "Bottle", "Notebook", "Pen", "Desk Lamp", "Chair",
    "Mug", "Planner", "Jacket", "Sneakers", "Watch", "Sunglasses",
    "Blanket", "Pillow", "Towel", "Candle", "Soap", "Lotion",
]

CATEGORIES = [
    "Electronics", "Clothing", "Home & Garden", "Sports", "Books",
    "Toys", "Health & Beauty", "Automotive", "Office Supplies",
    "Food & Beverage",
]

STATUSES = [
    "pending", "confirmed", "processing", "shipped", "delivered",
    "cancelled", "returned",
]

# Weighted toward USD
CURRENCIES = ["USD", "USD", "USD", "USD", "USD", "EUR", "GBP", "CAD", "AUD", "JPY"]

PAYMENT_METHODS = [
    "credit_card", "debit_card", "paypal", "apple_pay", "google_pay",
    "bank_transfer",
]

EMAIL_DOMAINS = [
    "gmail.com", "yahoo.com", "outlook.com", "hotmail.com", "icloud.com",
    "protonmail.com", "fastmail.com", "hey.com",
]

TAGS_POOL = [
    "flash-sale", "new-arrival", "clearance", "bestseller", "limited-edition",
    "eco-friendly", "gift-wrapped", "express-shipping", "bulk-order",
    "subscription", "holiday-special", "member-discount", "first-purchase",
    "returning-customer", "mobile-app", "desktop-web", "referral",
    "social-media", "email-campaign",
]

REFERRAL_SOURCES = [
    "google", "facebook", "instagram", "twitter", "tiktok", "youtube",
    "email", "direct", "affiliate", "friend", "blog", "podcast",
]

PROCESSOR_CODES = ["00", "05", "10", "14", "51", "54", "61"]
PROCESSOR_MESSAGES = [
    "Approved", "Do not honor", "Partial approval", "Invalid card number",
    "Insufficient funds", "Expired card", "Exceeds withdrawal limit",
]

# 80% null - realistic for an optional notes field
NOTES_TEMPLATES = [
    "Please leave at the front door",
    "Gift order - do not include receipt",
    "Fragile items - handle with care",
    "Deliver after 5pm",
    "Call before delivery",
    "Second floor apartment, buzz #{}",
    "Business address - delivery hours 9-5",
    "Leave with concierge",
    "Ring doorbell twice",
    "No signature required",
]

META_KEYS = [
    "source", "campaign", "device", "browser", "region", "experiment",
    "variant",
]
META_VALUES = [
    "web", "mobile", "api", "ios", "android", "chrome", "firefox", "safari",
    "us-east", "us-west", "eu-west", "apac", "control", "treatment-a",
    "treatment-b",
]

# Pre-compute product name combinations (18 * 30 = 540)
ALL_PRODUCT_NAMES = [
    f"{adj} {noun}" for adj in PRODUCT_ADJECTIVES for noun in PRODUCT_NOUNS
]

# ── Schema ───────────────────────────────────────────────────────────────────

ADDRESS_TYPE = pa.struct([
    pa.field("street", pa.string()),
    pa.field("city", pa.string()),
    pa.field("state", pa.string()),
    pa.field("zip", pa.string()),
    pa.field("country", pa.string()),
])

LINE_ITEM_TYPE = pa.struct([
    pa.field("product_id", pa.int32()),
    pa.field("product_name", pa.string()),
    pa.field("quantity", pa.int16()),
    pa.field("unit_price", pa.float64()),
    pa.field("category", pa.string()),
])

PROCESSOR_RESPONSE_TYPE = pa.struct([
    pa.field("code", pa.string()),
    pa.field("message", pa.string()),
    pa.field("risk_score", pa.float32()),
])

PAYMENT_TYPE = pa.struct([
    pa.field("method", pa.string()),
    pa.field("card_last_four", pa.string()),
    pa.field("processor_response", PROCESSOR_RESPONSE_TYPE),
])

SCHEMA = pa.schema([
    pa.field("order_id", pa.int64()),
    pa.field("user_id", pa.int32()),
    pa.field("order_date", pa.date32()),
    pa.field("created_at", pa.timestamp("us")),
    pa.field("updated_at", pa.timestamp("us")),
    pa.field("status", pa.string()),
    pa.field("is_priority", pa.bool_()),
    pa.field("is_returning_customer", pa.bool_()),
    pa.field("subtotal", pa.float64()),
    pa.field("tax_amount", pa.float32()),
    pa.field("discount_pct", pa.float64()),
    pa.field("total_amount", pa.float64()),
    pa.field("currency", pa.string()),
    pa.field("item_count", pa.int16()),
    pa.field("customer_name", pa.string()),
    pa.field("email", pa.string()),
    pa.field("age", pa.int8()),
    pa.field("loyalty_points", pa.uint32()),
    pa.field("weight_kg", pa.decimal128(10, 3)),
    pa.field("session_token", pa.binary(16)),
    pa.field("shipping_address", ADDRESS_TYPE),
    pa.field("billing_address", ADDRESS_TYPE),
    pa.field("tags", pa.list_(pa.string())),
    pa.field("ratings", pa.list_(pa.int8())),
    pa.field("line_items", pa.list_(LINE_ITEM_TYPE)),
    pa.field("payment", PAYMENT_TYPE),
    pa.field("metadata", pa.map_(pa.string(), pa.string())),
    pa.field("notes", pa.string()),
    pa.field("referral_source", pa.string()),
    pa.field("session_duration_ms", pa.int64()),
])

# Days from Unix epoch to 2023-01-01
BASE_DATE = (date(2023, 1, 1) - date(1970, 1, 1)).days
DATE_RANGE_DAYS = 365 * 3  # 3 years of data
US_PER_DAY = np.int64(86_400_000_000)


# ── Batch generation ─────────────────────────────────────────────────────────

def _build_address_arrays(rng, n):
    """Build parallel arrays for an address struct."""
    sn = rng.integers(0, len(STREET_NAMES), size=n)
    st = rng.integers(0, len(STREET_TYPES), size=n)
    nums = rng.integers(1, 9999, size=n)
    ci = rng.integers(0, len(CITIES), size=n)
    si = rng.integers(0, len(STATES), size=n)
    zp = rng.integers(10000, 99999, size=n)
    co = rng.integers(0, len(COUNTRIES), size=n)

    streets = [f"{nums[i]} {STREET_NAMES[sn[i]]} {STREET_TYPES[st[i]]}" for i in range(n)]
    cities = [CITIES[ci[i]] for i in range(n)]
    states = [STATES[si[i]] for i in range(n)]
    zips = [f"{zp[i]:05d}" for i in range(n)]
    countries = [COUNTRIES[co[i]] for i in range(n)]

    return [
        pa.array(streets),
        pa.array(cities),
        pa.array(states),
        pa.array(zips),
        pa.array(countries),
    ]


def generate_batch(rng, n, start_id):
    """Generate a single batch of n rows as a pyarrow RecordBatch."""

    # ── Scalar integers ──────────────────────────────────────────────────
    order_ids = np.arange(start_id, start_id + n, dtype=np.int64)
    user_ids = rng.integers(1_000_000, 10_000_000, size=n, dtype=np.int32)

    # ── Dates and timestamps ─────────────────────────────────────────────
    date_offsets = rng.integers(0, DATE_RANGE_DAYS, size=n)
    order_dates = (BASE_DATE + date_offsets).astype(np.int32)
    order_date_arr = pa.array(order_dates).cast(pa.date32())

    # created_at: same day as order_date, random time within day
    time_of_day_us = rng.integers(0, US_PER_DAY, size=n, dtype=np.int64)
    created_at_us = order_dates.astype(np.int64) * US_PER_DAY + time_of_day_us
    created_at_arr = pa.array(created_at_us).cast(pa.timestamp("us"))

    # updated_at: 0-30 days after created_at, 15% null
    update_offset_us = (
        rng.integers(0, 30, size=n, dtype=np.int64) * US_PER_DAY
        + rng.integers(0, US_PER_DAY, size=n, dtype=np.int64)
    )
    updated_at_us = created_at_us + update_offset_us
    updated_null = rng.random(n) < 0.15
    updated_at_arr = pa.array(updated_at_us, mask=updated_null).cast(
        pa.timestamp("us")
    )

    # ── String enums ─────────────────────────────────────────────────────
    status_idx = rng.integers(0, len(STATUSES), size=n)
    status_arr = pa.array([STATUSES[i] for i in status_idx])

    currency_idx = rng.integers(0, len(CURRENCIES), size=n)
    currency_arr = pa.array([CURRENCIES[i] for i in currency_idx])

    # ── Booleans ─────────────────────────────────────────────────────────
    is_priority_arr = pa.array(rng.random(n) < 0.10)
    is_returning_arr = pa.array(rng.random(n) < 0.40)

    # ── Monetary amounts ─────────────────────────────────────────────────
    subtotals = np.round(rng.uniform(5.0, 2000.0, size=n), 2)
    tax_rates = rng.uniform(0.0, 0.12, size=n)
    tax_amounts = np.round(subtotals * tax_rates, 2).astype(np.float32)

    has_discount = rng.random(n) < 0.30
    discount_pcts = np.where(
        has_discount, np.round(rng.uniform(0.05, 0.50, size=n), 2), 0.0
    )
    totals = np.round(
        subtotals * (1.0 - discount_pcts) + tax_amounts.astype(np.float64), 2
    )

    # ── Item count ───────────────────────────────────────────────────────
    item_counts = rng.integers(1, 12, size=n, dtype=np.int16)

    # ── Customer info ────────────────────────────────────────────────────
    first_idx = rng.integers(0, len(FIRST_NAMES), size=n)
    last_idx = rng.integers(0, len(LAST_NAMES), size=n)
    names = [
        f"{FIRST_NAMES[first_idx[i]]} {LAST_NAMES[last_idx[i]]}"
        for i in range(n)
    ]

    domain_idx = rng.integers(0, len(EMAIL_DOMAINS), size=n)
    email_nums = rng.integers(1, 999, size=n)
    emails = [
        f"{FIRST_NAMES[first_idx[i]].lower()}.{LAST_NAMES[last_idx[i]].lower()}"
        f"{email_nums[i]}@{EMAIL_DOMAINS[domain_idx[i]]}"
        for i in range(n)
    ]

    # age: int8, 10% null
    ages = rng.integers(18, 85, size=n, dtype=np.int8)
    age_null = rng.random(n) < 0.10
    age_arr = pa.array(ages, mask=age_null)

    # loyalty_points: uint32, 20% null
    points = rng.integers(0, 100_000, size=n).astype(np.uint32)
    points_null = rng.random(n) < 0.20
    points_arr = pa.array(points, mask=points_null)

    # weight_kg: decimal128(10,3)
    weights = np.round(rng.uniform(0.05, 75.0, size=n), 3)
    weight_arr = pa.array(
        [Decimal(f"{w:.3f}") for w in weights], type=pa.decimal128(10, 3)
    )

    # session_token: fixed_size_binary(16)
    token_bytes = rng.bytes(n * 16)
    tokens = [token_bytes[i * 16 : (i + 1) * 16] for i in range(n)]
    token_arr = pa.array(tokens, type=pa.binary(16))

    # ── Shipping address (struct, non-null) ──────────────────────────────
    ship_fields = _build_address_arrays(rng, n)
    shipping_arr = pa.StructArray.from_arrays(
        ship_fields,
        names=["street", "city", "state", "zip", "country"],
    )

    # ── Billing address (struct, 20% null = "same as shipping") ──────────
    bill_fields = _build_address_arrays(rng, n)
    billing_null = rng.random(n) < 0.20
    billing_arr = pa.StructArray.from_arrays(
        bill_fields,
        names=["street", "city", "state", "zip", "country"],
        mask=pa.array(billing_null),
    )

    # ── Tags: list<string>, 5% null ─────────────────────────────────────
    has_tags = rng.random(n) >= 0.05
    n_tags = np.where(has_tags, rng.integers(1, 5, size=n), 0)
    total_tags = int(n_tags.sum())
    flat_tag_idx = rng.integers(0, len(TAGS_POOL), size=max(total_tags, 1))

    tags_data = []
    pos = 0
    for i in range(n):
        nt = int(n_tags[i])
        if not has_tags[i]:
            tags_data.append(None)
        else:
            tags_data.append([TAGS_POOL[flat_tag_idx[pos + j]] for j in range(nt)])
            pos += nt
    tags_arr = pa.array(tags_data, type=pa.list_(pa.string()))

    # ── Ratings: list<int8>, 30% null ────────────────────────────────────
    has_ratings = rng.random(n) >= 0.30
    n_ratings = np.where(has_ratings, rng.integers(1, 6, size=n), 0)
    total_ratings = int(n_ratings.sum())
    flat_ratings = rng.integers(1, 6, size=max(total_ratings, 1), dtype=np.int8)

    ratings_data = []
    pos = 0
    for i in range(n):
        nr = int(n_ratings[i])
        if not has_ratings[i]:
            ratings_data.append(None)
        else:
            ratings_data.append(flat_ratings[pos : pos + nr].tolist())
            pos += nr
    ratings_arr = pa.array(ratings_data, type=pa.list_(pa.int8()))

    # ── Line items: list<struct> (using flat arrays + offsets) ────────────
    total_items = int(item_counts.sum())
    offsets = np.zeros(n + 1, dtype=np.int32)
    offsets[1:] = np.cumsum(item_counts.astype(np.int32))

    flat_pids = rng.integers(100_000, 1_000_000, size=total_items, dtype=np.int32)
    flat_pname_idx = rng.integers(0, len(ALL_PRODUCT_NAMES), size=total_items)
    flat_pnames = [ALL_PRODUCT_NAMES[i] for i in flat_pname_idx]
    flat_qty = rng.integers(1, 5, size=total_items, dtype=np.int16)
    flat_prices = np.round(rng.uniform(5.0, 500.0, size=total_items), 2)
    flat_cat_idx = rng.integers(0, len(CATEGORIES), size=total_items)
    flat_cats = [CATEGORIES[i] for i in flat_cat_idx]

    items_struct = pa.StructArray.from_arrays(
        [
            pa.array(flat_pids),
            pa.array(flat_pnames),
            pa.array(flat_qty),
            pa.array(flat_prices),
            pa.array(flat_cats),
        ],
        names=["product_id", "product_name", "quantity", "unit_price", "category"],
    )
    line_items_arr = pa.ListArray.from_arrays(pa.array(offsets), items_struct)

    # ── Payment: struct with nested struct ───────────────────────────────
    pm_idx = rng.integers(0, len(PAYMENT_METHODS), size=n)
    methods = [PAYMENT_METHODS[i] for i in pm_idx]
    card_nums = rng.integers(1000, 9999, size=n)
    card_last_four = [
        str(card_nums[i])
        if PAYMENT_METHODS[pm_idx[i]] in ("credit_card", "debit_card")
        else None
        for i in range(n)
    ]

    proc_idx = rng.integers(0, len(PROCESSOR_CODES), size=n)
    risk_scores = np.round(rng.uniform(0.0, 100.0, size=n), 1).astype(np.float32)

    proc_response = pa.StructArray.from_arrays(
        [
            pa.array([PROCESSOR_CODES[i] for i in proc_idx]),
            pa.array([PROCESSOR_MESSAGES[i] for i in proc_idx]),
            pa.array(risk_scores),
        ],
        names=["code", "message", "risk_score"],
    )
    payment_arr = pa.StructArray.from_arrays(
        [pa.array(methods), pa.array(card_last_four), proc_response],
        names=["method", "card_last_four", "processor_response"],
    )

    # ── Metadata: map<string, string> ────────────────────────────────────
    n_pairs = rng.integers(0, 4, size=n)
    total_pairs = int(n_pairs.sum())
    flat_mk_idx = rng.integers(0, len(META_KEYS), size=max(total_pairs, 1))
    flat_mv_idx = rng.integers(0, len(META_VALUES), size=max(total_pairs, 1))

    meta_data = []
    pos = 0
    for i in range(n):
        np_ = int(n_pairs[i])
        if np_ == 0:
            meta_data.append([])
        else:
            pairs = [
                (META_KEYS[flat_mk_idx[pos + j]], META_VALUES[flat_mv_idx[pos + j]])
                for j in range(np_)
            ]
            meta_data.append(pairs)
            pos += np_
    metadata_arr = pa.array(meta_data, type=pa.map_(pa.string(), pa.string()))

    # ── Notes: string, ~80% null ─────────────────────────────────────────
    has_notes = rng.random(n) < 0.20
    note_idx = rng.integers(0, len(NOTES_TEMPLATES), size=n)
    note_nums = rng.integers(100, 999, size=n)
    notes = [
        NOTES_TEMPLATES[note_idx[i]].format(note_nums[i]) if has_notes[i] else None
        for i in range(n)
    ]
    notes_arr = pa.array(notes, type=pa.string())

    # ── Referral source: string, ~20% null ───────────────────────────────
    has_ref = rng.random(n) >= 0.20
    ref_idx = rng.integers(0, len(REFERRAL_SOURCES), size=n)
    referrals = [
        REFERRAL_SOURCES[ref_idx[i]] if has_ref[i] else None for i in range(n)
    ]
    referral_arr = pa.array(referrals, type=pa.string())

    # ── Session duration ─────────────────────────────────────────────────
    session_ms = rng.integers(5_000, 1_800_000, size=n, dtype=np.int64)

    # ── Assemble batch ───────────────────────────────────────────────────
    return pa.RecordBatch.from_arrays(
        [
            pa.array(order_ids),
            pa.array(user_ids),
            order_date_arr,
            created_at_arr,
            updated_at_arr,
            status_arr,
            is_priority_arr,
            is_returning_arr,
            pa.array(subtotals),
            pa.array(tax_amounts),
            pa.array(discount_pcts),
            pa.array(totals),
            currency_arr,
            pa.array(item_counts),
            pa.array(names),
            pa.array(emails),
            age_arr,
            points_arr,
            weight_arr,
            token_arr,
            shipping_arr,
            billing_arr,
            tags_arr,
            ratings_arr,
            line_items_arr,
            payment_arr,
            metadata_arr,
            notes_arr,
            referral_arr,
            pa.array(session_ms),
        ],
        schema=SCHEMA,
    )


# ── Main ─────────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(
        description="Generate a realistic Parquet test file"
    )
    parser.add_argument(
        "-o", "--output", default="test_data.parquet", help="Output file path"
    )
    parser.add_argument(
        "-n", "--rows", type=int, default=10_000_000, help="Number of rows"
    )
    parser.add_argument(
        "--batch-size", type=int, default=500_000, help="Rows per batch"
    )
    parser.add_argument("--seed", type=int, default=42, help="Random seed")
    args = parser.parse_args()

    num_rows = args.rows
    batch_size = args.batch_size
    rng = np.random.default_rng(args.seed)

    print(f"Generating {num_rows:,} rows -> {args.output}")
    print(f"  Schema: {len(SCHEMA)} columns, batch size: {batch_size:,}")
    print()

    writer = pq.ParquetWriter(args.output, SCHEMA, compression="snappy", version="2.6")
    start = time.time()

    for batch_start in range(0, num_rows, batch_size):
        bs = min(batch_size, num_rows - batch_start)
        batch = generate_batch(rng, bs, batch_start)
        writer.write_batch(batch)

        rows_done = batch_start + bs
        elapsed = time.time() - start
        rate = rows_done / elapsed if elapsed > 0 else 0
        print(
            f"\r  {rows_done:>12,} / {num_rows:,} rows  "
            f"({rate:,.0f} rows/sec, {elapsed:.1f}s elapsed)",
            end="",
            flush=True,
        )

    writer.close()
    elapsed = time.time() - start

    import os

    size_mb = os.path.getsize(args.output) / (1024 * 1024)
    print(f"\n\nDone in {elapsed:.1f}s")
    print(f"  File: {args.output}")
    print(f"  Size: {size_mb:,.1f} MB")
    print(f"  Rows: {num_rows:,}")
    print(f"  Cols: {len(SCHEMA)}")


if __name__ == "__main__":
    main()
