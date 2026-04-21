.PHONY: examples
examples:
	cross build --target armv7-unknown-linux-gnueabihf --release --examples
