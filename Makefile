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
