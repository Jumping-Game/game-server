.PHONY: build test bench lint fmt

build:
cargo build --workspace

test:
cargo test --workspace

bench:
cargo bench --workspace -- --warm-up-time 0

lint:
cargo clippy --workspace -- -D warnings

fmt:
cargo fmt --all
