build:
	cargo build

install:
	cargo install --force --path .

.PHONY: build install
