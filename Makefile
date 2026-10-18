## Monorepo root. Every target delegates; nothing is built here.
##
## The two trees keep separate build systems because they cannot share one. `smart-contract/` is
## a Cargo workspace targeting wasm32v1-none with an aggressive `[profile.release]`;
## `backend/` is a native Cargo workspace with an ordinary dependency graph. Merging them would
## silently drop the contract's profile — it only takes effect at a workspace root — and the wasm
## would grow and lose its overflow checks with nothing failing to say so.

.PHONY: all contract backend test lint fmt clean build coverage deploy demo verify help

CONTRACT := smart-contract
BACKEND  := backend

help:
	@echo "make all        fmt, lint, test and build both trees"
	@echo "make contract   the Soroban contract only"
	@echo "make backend    the RPC indexer only"
	@echo "make test       every test in the repository"
	@echo "make coverage   line coverage over the contract, for the evidence package"
	@echo "make deploy     deploy one treasury to testnet      (KEY=<identity> optional)"
	@echo "make demo       the acceptance scenario, end to end (KEY=<identity> optional)"
	@echo "make verify     check the committed evidence against the chain — no key, read-only"
	@echo
	@echo "on testnet:  make demo                 creates and funds sx-deployer if needed"
	@echo "your own:    KEY=<alias> make demo     required on NETWORK=public"

all: contract backend

contract:
	$(MAKE) -C $(CONTRACT) all

backend:
	$(MAKE) -C $(BACKEND) all

build:
	$(MAKE) -C $(CONTRACT) build
	$(MAKE) -C $(BACKEND) build

test:
	$(MAKE) -C $(CONTRACT) test
	$(MAKE) -C $(BACKEND) test

lint:
	$(MAKE) -C $(CONTRACT) lint
	$(MAKE) -C $(BACKEND) lint

fmt:
	$(MAKE) -C $(CONTRACT) fmt
	$(MAKE) -C $(BACKEND) fmt

clean:
	$(MAKE) -C $(CONTRACT) clean
	$(MAKE) -C $(BACKEND) clean

## Deploy one treasury for one organisation. Multi-organisation tenancy is out of scope, so this
## is run once per org rather than there being a factory.
deploy:
	KEY=$(KEY) $(MAKE) -C $(CONTRACT) deploy

## The acceptance scenario: clone on a clean machine, run one command, get every transaction
## hash. If this does not complete, Deliverable 3 is not complete regardless of other evidence.
demo:
	KEY=$(KEY) $(MAKE) -C $(CONTRACT) demo

## How much of the contract the suite actually reaches. Written to
## smart-contract/.deploy/coverage.txt, which goes in the evidence package alongside the test log
## — a passing suite says nothing about what it never ran.
coverage:
	$(MAKE) -C $(CONTRACT) coverage

## Everything a reviewer is given, in one command.

## Audit the committed evidence package against the live chain.
##
## Deliberately not the same code path as `demo`: the demo is the party making the claim, this is
## the party checking it. It needs no key, deploys nothing and changes nothing — so it is the one
## to run live in front of an audience, as often as you like.
verify:
	@./scripts/verify-evidence.sh
