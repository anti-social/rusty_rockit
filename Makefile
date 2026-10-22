.PHONY: examples host host-test
examples:
	cross build --target armv7-unknown-linux-gnueabihf --release --examples

# Build and test on a non-ARM host against the fake Rockit library
host:
	cargo build --examples

host-test:
	cargo test --workspace
