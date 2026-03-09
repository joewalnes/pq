.PHONY: all build test lint install

all: build test lint

build:
	cargo build --release

test:
	cargo test --workspace

lint:
	cargo clippy --workspace -- -D warnings
	cargo fmt --all -- --check

install: build
	mkdir -p ~/.local/bin
	cp target/release/pq ~/.local/bin/pq

# -- Sample data -----------------------------------------------------------
# Downloads a variety of public parquet files for manual testing.
# All files land in data/ which is gitignored.

DATA_DIR := data
PARQUET_TESTING := https://raw.githubusercontent.com/apache/parquet-testing/master/data

.PHONY: sample-data clean-data

sample-data: $(DATA_DIR)/.stamp

$(DATA_DIR)/.stamp:
	mkdir -p $(DATA_DIR)
	@echo "==> NYC taxi (yellow, Jan 2024 — ~45 MB, flat schema)"
	curl -fSL -o $(DATA_DIR)/nyc-taxi-2024-01.parquet \
		"https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet"
	@echo "==> NYC taxi (green, Jan 2024 — ~1.5 MB, flat schema)"
	curl -fSL -o $(DATA_DIR)/nyc-taxi-green-2024-01.parquet \
		"https://d37ci6vzurychx.cloudfront.net/trip-data/green_tripdata_2024-01.parquet"
	@echo "==> Nested lists (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/nested_lists.parquet \
		"$(PARQUET_TESTING)/nested_lists.snappy.parquet"
	@echo "==> Nested maps (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/nested_maps.parquet \
		"$(PARQUET_TESTING)/nested_maps.snappy.parquet"
	@echo "==> Nested structs (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/nested_structs.parquet \
		"$(PARQUET_TESTING)/nested_structs.rust.parquet"
	@echo "==> List columns (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/list_columns.parquet \
		"$(PARQUET_TESTING)/list_columns.parquet"
	@echo "==> All types + nulls (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/alltypes_plain.parquet \
		"$(PARQUET_TESTING)/alltypes_plain.parquet"
	@echo "==> All types snappy (apache/parquet-testing)"
	curl -fSL -o $(DATA_DIR)/alltypes_plain.snappy.parquet \
		"$(PARQUET_TESTING)/alltypes_plain.snappy.parquet"
	@touch $@

clean-data:
	rm -rf $(DATA_DIR)
