.PHONY: examples host-stub host host-test
examples:
	cross build --target armv7-unknown-linux-gnueabihf --release --examples

# Build and test on a non-ARM host against the fake librockit.so
host-stub:
	cargo build -p rockit-stub

host: host-stub
	cargo build --examples

host-test: host-stub
	cargo test --workspace
