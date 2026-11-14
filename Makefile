.PHONY: check test build install

# What CI's lint job runs.
check:
	cargo fmt --check
	RUSTFLAGS=-Dwarnings cargo clippy --all-targets --all-features -- -D warnings
	RUSTDOCFLAGS=-Dwarnings cargo doc --all-features --no-deps

# Offline: Vultr and the IP services are wiremock servers.
test:
	cargo test --locked

build:
	cargo build --release --features binary-release

install:
	cargo install --path . --locked
