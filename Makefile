BUILD_VERSION = $(shell date -u +'%Y-%m-%d %H:%M') $(shell git rev-parse --abbrev-ref HEAD) $(shell git rev-parse --short HEAD) dev

.PHONY: all build test test-golden test-integration lint install docs docs-serve \
       release clean-release example-data upload-examples

all: build test lint

build:
	BUILD_VERSION="$(BUILD_VERSION)" cargo build --release

test:
	cargo test --workspace
	@$(MAKE) test-golden

test-golden: build
	PQ=target/release/pq python3 tests/golden/run.py

lint:
	cargo clippy --workspace -- -D warnings
	cargo fmt --all -- --check

install: build
	mkdir -p ~/.local/bin
	cp target/release/pq ~/.local/bin/pq

# -- Third-party license bundling --------------------------------------------
# Regenerates THIRD-PARTY-LICENSES at the repo root from the workspace's
# dependency graph via cargo-about (https://github.com/EmbarkStudios/cargo-about),
# using about.toml/about.hbs for config/formatting. The file is NOT committed:
# it is ~500KB and tracks Cargo.lock exactly, so it would go stale the moment
# a dependency version bumps without anyone noticing. Regenerate it with
# `make licenses` before packaging a release, which also copies it (and
# pq's own LICENSE) into each npm package directory so `npm pack`/`npm
# publish` picks them up (pypi/build_wheels.py instead reads both files
# directly from the repo root and embeds them in the wheel itself).
#
# Also appends verbatim any dependency NOTICE files (Apache-2.0 section 4(d)
# requires preserving these; cargo-about only handles LICENSE text, not NOTICE
# files, so that part is done by hand below).
#
# NOTE: wiring `make licenses` into .github/workflows/release.yml so it runs
# automatically before packaging is deliberately NOT done here - that file is
# owned by another in-flight change to the release workflow. Until that
# lands, a human must run `make licenses` before cutting a release.

.PHONY: licenses

define NOTICE_APPENDIX_PY
import json, os, sys

about_json_path, out_path = sys.argv[1], sys.argv[2]
data = json.load(open(about_json_path))

notices = {}  # NOTICE content -> set of "name version" crate ids that ship it
for c in data["crates"]:
    manifest = c["package"].get("manifest_path")
    if not manifest:
        continue
    crate_dir = os.path.dirname(manifest)
    for fname in ("NOTICE", "NOTICE.txt", "NOTICE.md"):
        p = os.path.join(crate_dir, fname)
        if os.path.isfile(p):
            content = open(p, errors="replace").read().strip()
            notices.setdefault(content, set()).add(
                f"{c['package']['name']} {c['package']['version']}"
            )
            break

if not notices:
    sys.exit(0)

with open(out_path, "a") as out:
    out.write("\n" + "=" * 80 + "\n\n")
    out.write("Notices (Apache License 2.0 section 4(d))\n")
    out.write("-------------------------------------------\n\n")
    out.write(
        "The following crates ship a NOTICE file which Apache-2.0 section 4(d)\n"
        "requires be preserved in derivative works. Reproduced verbatim below.\n\n"
    )
    for content, crates in sorted(notices.items(), key=lambda kv: sorted(kv[1])):
        out.write("-" * 80 + "\nUsed by:\n")
        for name in sorted(crates):
            out.write(f"  - {name}\n")
        out.write("\n" + content + "\n\n")

print(f"licenses: appended {len(notices)} unique NOTICE text(s) covering "
      f"{sum(len(v) for v in notices.values())} crate(s)", file=sys.stderr)
endef
export NOTICE_APPENDIX_PY

LICENSE_DEST_DIRS := npm/darwin-arm64 npm/linux-arm64 npm/linux-x64 npm/pqtool

licenses:
	@cargo about --version >/dev/null 2>&1 || { \
		echo "error: cargo-about is not installed (or not runnable via 'cargo about')."; \
		echo "  install with: cargo install cargo-about --locked --features cli"; \
		exit 1; \
	}
	@set -e; \
	tmp=$$(mktemp -t pq-about-json); \
	trap 'rm -f "$$tmp"' EXIT; \
	cargo about generate --workspace -o THIRD-PARTY-LICENSES about.hbs; \
	cargo about generate --workspace --format json -o "$$tmp"; \
	python3 -c "$$NOTICE_APPENDIX_PY" "$$tmp" THIRD-PARTY-LICENSES; \
	for d in $(LICENSE_DEST_DIRS); do \
		cp LICENSE THIRD-PARTY-LICENSES $$d/; \
	done; \
	echo "licenses: wrote THIRD-PARTY-LICENSES ($$(wc -l < THIRD-PARTY-LICENSES) lines) and copied LICENSE + THIRD-PARTY-LICENSES into: $(LICENSE_DEST_DIRS)"; \
	echo "licenses: pypi/build_wheels.py reads LICENSE/THIRD-PARTY-LICENSES from the repo root directly - no copy needed there."

# -- Documentation ------------------------------------------------------------

docs: build demos
	PQ=target/release/pq ./docs/generate-cli-reference.sh
	python3 docs/build.py

docs-serve: build demos
	PQ=target/release/pq ./docs/generate-cli-reference.sh
	python3 docs/build.py --serve


# -- Demo GIFs -------------------------------------------------------------
# Each demos/*.py script produces a GIF in docs/build/img/.
# Incremental: only re-records when the script, driver, or binary changes.

DEMO_SCRIPTS := $(wildcard demos/*.py)
DEMO_DRIVER  := demos/driver.py demos/record.sh
DEMO_GIFS    := $(patsubst demos/%.py,docs/build/img/%.gif,$(filter-out demos/driver.py,$(DEMO_SCRIPTS)))

.PHONY: demos

demos: build $(DEMO_GIFS)
	@echo "$(words $(DEMO_GIFS)) demo GIF(s) up to date"

docs/build/img/%.gif: demos/%.py $(DEMO_DRIVER) target/release/pq
	@mkdir -p docs/build/img
	./demos/record.sh $< $@

# -- Release binaries ------------------------------------------------------
# Produces static binaries in dist/.
#   make release                - all platforms
#   make release-darwin-arm64   - macOS Apple Silicon only
#   make release-linux-amd64    - Linux x86_64 only
#   make release-linux-arm64    - Linux ARM64 (Graviton) only
#
# Linux targets use musl for fully static binaries. cross is installed
# automatically if missing. Docker must be running for Linux builds.

DIST_DIR := dist
CROSS := $(HOME)/.cargo/bin/cross
RUST_SOURCES := $(shell find crates Cargo.toml Cargo.lock -name '*.rs' -o -name 'Cargo.toml' -o -name 'Cargo.lock' 2>/dev/null)

.PHONY: release clean-release

release: $(DIST_DIR)/pq-darwin-arm64/pq $(DIST_DIR)/pq-linux-amd64/pq $(DIST_DIR)/pq-linux-arm64/pq

$(CROSS):
	cargo install cross --git https://github.com/cross-rs/cross

$(DIST_DIR)/pq-darwin-arm64/pq: $(RUST_SOURCES)
	rustup target add aarch64-apple-darwin
	cargo build --release --target aarch64-apple-darwin
	@mkdir -p $(DIST_DIR)/pq-darwin-arm64
	cp target/aarch64-apple-darwin/release/pq $@

$(DIST_DIR)/pq-linux-amd64/pq: $(RUST_SOURCES) | $(CROSS)
	$(CROSS) build --release --target x86_64-unknown-linux-musl
	@mkdir -p $(DIST_DIR)/pq-linux-amd64
	cp target/x86_64-unknown-linux-musl/release/pq $@

$(DIST_DIR)/pq-linux-arm64/pq: $(RUST_SOURCES) | $(CROSS)
	$(CROSS) build --release --target aarch64-unknown-linux-musl
	@mkdir -p $(DIST_DIR)/pq-linux-arm64
	cp target/aarch64-unknown-linux-musl/release/pq $@

clean-release:
	rm -rf $(DIST_DIR)

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
#   make test-integration     # full lifecycle
#   make test-seaweed-up      # start container only
#   make test-seaweed-down    # stop container only

SEAWEED_CONTAINER := pq-seaweed-test
SEAWEED_S3_PORT   := 8333
SEAWEED_FILER_PORT := 8888
SEAWEED_S3_KEY    := testkey
SEAWEED_S3_SECRET := testsecret
SEAWEED_S3_CONF   := /tmp/pq-seaweed-s3.json

.PHONY: test-integration test-seaweed-up test-seaweed-down

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

test-integration: test-seaweed-up
	cargo test --test remote_tests -- --ignored; \
	status=$$?; \
	$(MAKE) test-seaweed-down; \
	exit $$status

# -- Example data for public download (R2) ------------------------------------
# Generates two parquet files and uploads to Cloudflare R2.
#   make example-data      - generate files locally
#   make upload-examples   - generate + upload to R2

EXAMPLE_DIR    := data/examples
EXAMPLE_TINY   := $(EXAMPLE_DIR)/orders-10k.parquet
EXAMPLE_SMALL  := $(EXAMPLE_DIR)/orders-100k.parquet
EXAMPLE_LARGE  := $(EXAMPLE_DIR)/orders-100m.parquet
R2_BUCKET      := pq-example-data
R2_URL         := https://data.pqtool.dev

example-data: $(EXAMPLE_TINY) $(EXAMPLE_SMALL) $(EXAMPLE_LARGE)

$(EXAMPLE_TINY):
	@mkdir -p $(EXAMPLE_DIR)
	python3 generate_test_parquet.py -n 10000 -o $@

$(EXAMPLE_SMALL):
	@mkdir -p $(EXAMPLE_DIR)
	python3 generate_test_parquet.py -n 100000 -o $@

$(EXAMPLE_LARGE):
	@mkdir -p $(EXAMPLE_DIR)
	python3 generate_test_parquet.py -n 100000000 -o $@

upload-examples: example-data
	npx wrangler r2 object put $(R2_BUCKET)/orders-10k.parquet --file $(EXAMPLE_TINY) --content-type application/octet-stream --remote
	npx wrangler r2 object put $(R2_BUCKET)/orders-100k.parquet --file $(EXAMPLE_SMALL) --content-type application/octet-stream --remote
	npx wrangler r2 object put $(R2_BUCKET)/orders-100m.parquet --file $(EXAMPLE_LARGE) --content-type application/octet-stream --remote
	@echo ""
	@echo "Uploaded to R2 bucket '$(R2_BUCKET)'."
	@echo "Public URL: $(R2_URL)/"
	@echo "  orders-10k.parquet   (~2 MB, tiny)"
	@echo "  orders-100k.parquet  (~20 MB, fast download)"
	@echo "  orders-100m.parquet  (~10 GB, lazy loading demo)"
