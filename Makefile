build:
	cargo build

install:
	cargo install --force --path .

publish:
	cargo publish

.PHONY: build install
