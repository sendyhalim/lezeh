build:
	cargo build

install:
	cargo install --force --path .

publish:
	cargo publish --package lezeh

.PHONY: build install
