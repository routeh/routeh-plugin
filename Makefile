.PHONY: test lint coverage

# Cargo's default targets: the tests, and the examples built as programs
# (the SDK's tests run its example as a plugin's service). `--examples`
# would build them as test harnesses instead, and `--all-targets` an
# empty harness for every library that says `test = false`.
test:
	cargo test --workspace --locked

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --locked -- -D warnings

# Every crate has a gate of its own. Of routeh-plugin-build, what runs
# Docker, the SDK and QEMU (cli, cmd, docker, make, openwrt, qemu,
# scripts) is proved by building a plugin, not here: not measured.
# The tests run in llvm-cov's environment (`show-env`) with Cargo's
# default targets, so the example they run is built as a program and
# instrumented too: `cargo llvm-cov` itself builds only `--tests`. A
# target directory of its own keeps instrumented builds out of `make test`.
coverage:
	mkdir -p coverage
	export CARGO_TARGET_DIR="$(CURDIR)/target/llvm-cov-target" && \
	eval "$$(cargo llvm-cov show-env --sh)" && \
	cargo llvm-cov clean --workspace && \
	cargo test --workspace --locked && \
	cargo llvm-cov report --ignore-filename-regex '/tests/|/examples/|build/src/(bin/|cli|cmd|docker|make|openwrt|qemu|scripts)' --fail-under-lines 90 --json --summary-only --output-path coverage/workspace.json && \
	cargo llvm-cov report -p routeh-plugin-protocol --ignore-filename-regex '/tests/' --fail-under-lines 90 && \
	cargo llvm-cov report -p routeh-plugin-sdk --ignore-filename-regex '/tests/|/examples/' --fail-under-lines 90 && \
	cargo llvm-cov report -p routeh-plugin-bundle --ignore-filename-regex '/tests/' --fail-under-lines 90 && \
	cargo llvm-cov report -p routeh-plugin-build --ignore-filename-regex '/tests/|build/src/(bin/|cli|cmd|docker|make|openwrt|qemu|scripts)' --fail-under-lines 90
