.PHONY: all build test fmt lint clean coverage deploy demo

NETWORK ?= testnet
TARGET  := wasm32v1-none
WASM    := target/$(TARGET)/release/saintix_treasury.wasm

all: fmt lint test build

## --workspace, not just the root package. mock-token is only a DEV-dependency of the treasury,
## so a plain `cargo build` never produces its wasm — and scripts/deploy.sh needs it to stand in
## for USDC on a machine that holds none.
build:
	cargo build --workspace --target $(TARGET) --release
	@ls -lh $(WASM) | awk '{print "wasm: " $$5}'

test:
	cargo test --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

## Removes the build scratch and LEAVES THE EVIDENCE PACKAGE. `rm -rf .deploy` would delete the
## deliverable now that it lives there — the four committed files are named explicitly rather than
## the directory being blown away.
clean:
	cargo clean
	rm -f .deploy/env .deploy/index.json .deploy/lcov.info

## Line coverage over the contract's own source, for the evidence package.
##
## The test modules are excluded from the measurement. Counting them inflates the figure with
## code that is by definition executed, and the question a reviewer is asking is how much of the
## CONTRACT the suite reaches — mock-token is a fixture and goes out for the same reason.
##
## Both land in .deploy/, but only one is the deliverable: the readable summary is committed with
## the rest of the evidence package, and the lcov file a tool consumes is ignored scratch. Needs
## cargo-llvm-cov:
##   cargo install cargo-llvm-cov --locked
coverage:
	@command -v cargo-llvm-cov >/dev/null 2>&1 \
		|| { echo "cargo-llvm-cov is not installed. Run: cargo install cargo-llvm-cov --locked"; exit 1; }
	@mkdir -p .deploy
	cargo llvm-cov --workspace \
		--ignore-filename-regex '(src/test/|mock-token/)' \
		--lcov --output-path .deploy/lcov.info
	@cargo llvm-cov report \
		--ignore-filename-regex '(src/test/|mock-token/)' \
		| tee .deploy/coverage.txt

## Deployment and construction happen in ONE transaction; splitting them leaves a window in
## which somebody else can initialise the contract and name themselves admin.
## `stellar contract deploy -- <constructor args>` is the form that guarantees it.
deploy:
	KEY=$(KEY) NETWORK=$(NETWORK) ./scripts/deploy.sh

## Clone on a clean machine, run one command, get every transaction hash.
##
## Every refusal asserts its exact code and fails the script on a mismatch — without that, a step
## failing for the WRONG reason still passes and the evidence package becomes a set of
## transactions nobody verified. See ../docs/10-testing.md.
##
## Takes a few minutes: there is no wall-clock wait anywhere in this contract, only ledger
## round trips.
demo:
	KEY=$(KEY) NETWORK=$(NETWORK) ./scripts/demo.sh
