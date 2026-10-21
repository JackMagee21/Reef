# Shortcuts for working on Reef. Every recipe is a plain cargo command, so this
# works the same whether make runs it through cmd.exe (Windows) or sh (Linux).

LINUX_TARGET = x86_64-unknown-linux-gnu
CMD = echo hello from reef

.PHONY: help run c build release test check fmt clean

help:
	@echo Usage: make TARGET
	@echo   run       start the interactive reef prompt
	@echo   c         run one command:  make c CMD="echo hi"
	@echo   build     debug build into target/debug
	@echo   release   optimized build into target/release
	@echo   test      run the tests
	@echo   check     everything CI runs: fmt, clippy for Windows and Linux, tests
	@echo   fmt       format the code
	@echo   clean     delete build output

run:
	cargo run

c:
	cargo run -- -c "$(CMD)"

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

check:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo clippy --target $(LINUX_TARGET) --all-targets -- -D warnings
	cargo test

fmt:
	cargo fmt

clean:
	cargo clean
