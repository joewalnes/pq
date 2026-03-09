#!/usr/bin/env python3
"""Generate test parquet files for pq integration tests."""
import json
import os

# Generate JSONL that we can convert with pq convert
data = []
for i in range(100):
    data.append({
        "id": i,
        "name": f"user_{i}",
        "age": 20 + (i % 50),
        "score": round(i * 1.5, 2),
        "active": i % 3 != 0,
        "city": ["New York", "London", "Tokyo", "Paris", "Berlin"][i % 5]
    })

output_dir = os.path.dirname(os.path.abspath(__file__))
jsonl_path = os.path.join(output_dir, "test_data.jsonl")

with open(jsonl_path, "w") as f:
    for row in data:
        f.write(json.dumps(row) + "\n")

print(f"Wrote {len(data)} rows to {jsonl_path}")
