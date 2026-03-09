.PHONY: all build test test-remote lint install

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

# -- Integration tests (SeaweedFS) -----------------------------------------
# Spins up a SeaweedFS container with S3 + filer, runs remote_tests, tears down.
#
#   make test-remote          # full lifecycle
#   make test-seaweed-up      # start container only
#   make test-seaweed-down    # stop container only

SEAWEED_CONTAINER := pq-seaweed-test
SEAWEED_S3_PORT   := 8333
SEAWEED_FILER_PORT := 8888
SEAWEED_S3_KEY    := testkey
SEAWEED_S3_SECRET := testsecret
SEAWEED_S3_CONF   := /tmp/pq-seaweed-s3.json

.PHONY: test-remote test-seaweed-up test-seaweed-down

test-seaweed-up:
	@printf '{"identities":[{"name":"testuser","credentials":[{"accessKey":"%s","secretKey":"%s"}],"actions":["Admin","Read","Write","List","Tagging"]}]}\n' \
		$(SEAWEED_S3_KEY) $(SEAWEED_S3_SECRET) > $(SEAWEED_S3_CONF)
	@docker rm -f $(SEAWEED_CONTAINER) 2>/dev/null || true
	docker run -d --name $(SEAWEED_CONTAINER) \
		-p $(SEAWEED_S3_PORT):8333 \
		-p $(SEAWEED_FILER_PORT):8888 \
		-v $(SEAWEED_S3_CONF):/etc/s3.json:ro \
		chrislusf/seaweedfs server -s3 -s3.config=/etc/s3.json
	@echo "Waiting for SeaweedFS to start..."
	@for i in 1 2 3 4 5 6 7 8 9 10; do \
		curl -sf http://localhost:$(SEAWEED_S3_PORT)/ >/dev/null 2>&1 && break; \
		sleep 1; \
	done
	@echo "SeaweedFS ready (S3=:$(SEAWEED_S3_PORT), filer=:$(SEAWEED_FILER_PORT))"

test-seaweed-down:
	docker rm -f $(SEAWEED_CONTAINER) 2>/dev/null || true
	@rm -f $(SEAWEED_S3_CONF)

test-remote: test-seaweed-up
	cargo test --test remote_tests -- --ignored; \
	status=$$?; \
	$(MAKE) test-seaweed-down; \
	exit $$status
