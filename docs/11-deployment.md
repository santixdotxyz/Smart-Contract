# 11 · Deployment and the evidence package

## Prerequisites

```
rust        1.97.1, with the wasm32v1-none target   (rust-toolchain.toml pins it)
stellar     the Stellar CLI, 22 or newer
jq          the scripts parse contract output with it
curl        the scripts read the current ledger straight from RPC
```

A funded testnet identity — optional, since a testnet run generates and funds `sx-deployer` for
itself when `KEY` is unset:

```bash
stellar keys generate --fund sx-deployer --network testnet
```

## Build and check

```bash
make all           # fmt, clippy -D warnings, test, build — both trees
make test          # 113 contract tests + 18 indexer tests
make coverage      # → smart-contract/.deploy/coverage.txt
```

`make all` fails on a clippy warning. That is deliberate: the alternative is a warning nobody reads accumulating until the one that matters is invisible among them.

## Providing the signing key

`deploy` and `demo` submit real transactions, so they need something that can **sign**. That is
`KEY`, and the Stellar CLI resolves either form — the scripts never parse or store a key
themselves.

| | `KEY=` | Where the secret lives |
|---|---|---|
| An identity **alias** | `sx-deployer` | `~/.config/stellar/identity/sx-deployer.toml`, mode `0600` |
| A raw **secret key** | `SC36…` (56 chars, leading `S`) | Nowhere on disk — only that command's environment |
| **Nothing at all** | *unset* | Test networks only: `sx-deployer` is generated and funded, and `preflight` announces it |

```bash
# on testnet, leave it out entirely — a signer is created and funded if none exists
make demo

# the usual way
stellar keys generate --fund sx-deployer --network testnet
KEY=sx-deployer make demo

# injecting a secret instead — CI, or a machine you would rather not write keys to
read -rs KEY && export KEY        # paste; nothing is echoed or kept in shell history
make demo
```

The account must be **funded**: it pays fees for two contract deployments and thirty steps.
`--fund` asks friendbot; `stellar keys fund <alias> --network testnet` tops one up later.

### `preflight` refuses before it submits anything

The checks are in `scripts/lib.sh`, and they run before the first transaction:

| Passed | Result |
|---|---|
| nothing, on a test network | `sx-deployer` generated and funded, and announced |
| nothing, on `public` | refused, printing both ways to supply it |
| a **public** key `G…` | refused — *"it identifies an account but cannot sign for it"* |
| a muxed address `M…` | refused |
| an unknown alias | refused, with the `stellar keys generate` line to run |

The public-key case earns its own check because it is the likeliest mistake — a keypair has two
halves and only one of them signs — and because without it the run got as far as the first deploy
before failing with an error about the transaction rather than about `KEY`.

An **unfunded** account is the one case that cannot be caught up front without a network round
trip; it fails at the first deploy, on fees.

### The demo's own keys

`demo.sh` needs five participants with distinct roles — admin, operator, outsider, settlement,
rogue — because a guard is only proven by somebody who is *not* allowed past it. It generates and
funds them itself (`sx_admin`, `sx_operator`, …) into `~/.config/stellar/identity/`, and reuses
them on later runs. `stellar keys rm sx_admin` removes one.

### Security

These are **testnet** keys: friendbot supplies the XLM, the default token is a fixture anyone can
mint, and Phase 1 has no mainnet. A leaked testnet deployer key costs a re-run and nothing else.

**Never point `KEY` at an account holding real funds.** Nothing here needs one and no path uses
one. A secret passed on a command line is visible to every process via `ps` and is written to
shell history — `read -rs`, or an environment variable from a secret store, avoids both.

## Deploy one treasury

```bash
KEY=sx-deployer make deploy
```

**One instance per organisation.** Multi-organisation tenancy is out of scope, so there is no factory and no registry — this is run once per org, and two instances know nothing about each other.

### Construction happens in the deploy transaction

```bash
stellar contract deploy --wasm … -- \
    --admin <G…> --token_addr <C…> --operators '["<G…>"]'
```

Deploying first and initialising second leaves a window — a few seconds, entirely sufficient — in which anybody watching the ledger can call `initialize` and name themselves admin of a contract somebody else paid to deploy. `deploy.sh` uses the constructor form for that reason and nothing else.

### The token

By default `deploy.sh` deploys `mock-token` as a stand-in USDC, because a self-contained demo cannot depend on the reader holding testnet USDC or establishing a trustline first.

Point it at the real thing with:

```bash
TOKEN=CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA \
KEY=sx-deployer make deploy
```

The contract reads `decimals` from whatever it is given and never trusts the deployer's claim about it, so both paths are the same code.

**`mock-token` must never reach mainnet.** Anyone can mint it and anyone can claw it back — catastrophic for a real token and exactly right for a fixture one. The treasury takes its token by address and never links against the crate, so the only thing standing between it and a real deployment is not deploying it.

### It verifies itself against the chain

`deploy.sh` reads the configuration back with `get_config` rather than echoing what it passed in. A deploy that silently used different arguments than intended looks identical from the outside otherwise.

### What it writes

`.deploy/env`, sourced by `demo.sh`:

```bash
export TREASURY="C…"        export TOKEN="C…"
export ADMIN="G…"           export OPERATOR="G…"
export NETWORK="testnet"    export RPC_URL="…"
export DEPLOY_LEDGER="…"    # where the contract's history begins
```

`DEPLOY_LEDGER` is noted **before** anything is deployed, with a few ledgers of margin. The indexer needs somewhere to start reading events from; starting at the node's oldest retained ledger is correct and walks through a hundred thousand ledgers of unrelated traffic to find a contract deployed a minute ago. Same answer, seconds instead of minutes.

## The acceptance scenario

```bash
KEY=sx-deployer make demo
```

Thirty steps on testnet: deposit → four cards → allocate → deallocate → rebalance → limits → freeze → three approvals → three recorded declines → two reverting refusals → whitelist → settle → four settlement refusals → settle again → the indexer's rebuild and confirmation.

A few minutes. There is no wall-clock wait anywhere in this contract — only ledger round trips.

### The property that makes it evidence

**Every refusal asserts its exact code and fails the run on a mismatch.**

Without that, a step that fails for the *wrong* reason still passes: the script goes green, `evidence.md` fills up, and nobody has verified anything. That is worse than no evidence, because it looks like evidence.

Two helpers, because this contract has two families of refusal:

```bash
call 6 "allocate past the pool" -- …
    # expects Error(Contract, #6). Never reaches the ledger — the CLI simulates
    # before submitting. Recorded as `simulation`, never given a fabricated hash.

call_decision Declined:LimitExceeded "18,900 against 13,000 left" -- …
    # expects a SUCCESSFUL transaction carrying that reason code.
    # Gets a real hash, because a declined spend really does reach the ledger.
```

The hash is parsed from the CLI's own `Signing transaction:` label rather than by scanning for 64 hex characters. Several things in this contract's arguments are long hex strings, and a loose match records one of those as the transaction hash — producing an evidence file that looks entirely plausible and opens to nothing.

### And the off-chain half is asserted, not trusted

```bash
expect_eq "settlements the indexer rebuilt"    "$(jq '.settlements | length' "$IDX")" "2"
expect_eq "settlements it confirmed"           "…"                                     "2"
expect_eq "spend records the indexer rebuilt"  "$(jq '.spends | length' "$IDX")"       "6"
expect_eq "declines the indexer rebuilt"       "…"                                     "3"
expect_eq "cards the indexer rebuilt"          "$(jq '.cards | length' "$IDX")"        "5"
expect_eq "total_settled, from events alone"   "$(jq -r '.treasury.total_settled')"    "…"
```

An indexer that read nothing exits zero and writes a well-formed state file full of zeroes. These six lines compare two **independent** views of the same treasury — the contract's view functions, and the fold over its event stream — and agreement between them is the whole claim module M13 makes.

They were added after exactly that failure: a paging bug made the indexer return nothing, and the demo went green anyway. See [09-offchain-integration.md](09-offchain-integration.md).

## The evidence package

```bash
KEY=sx-deployer make demo          # evidence.md + reconciliation.md
make coverage                      # coverage.txt — no key, no network
```

| File | What it is |
|---|---|
| `smart-contract/.deploy/evidence.md` | One row per step: number, description, error or reason code, transaction hash linked to Stellar Expert. |
| `smart-contract/.deploy/reconciliation.md` | The same treasury rebuilt **independently** from its event stream, as a table in whole USDC, every settlement linked to the spend it closed. |
| `smart-contract/.deploy/coverage.txt` | How much of the contract the suite actually reaches. |
| `README.md` | The evidence table, organised by D1/D2/D3, so §6.2 can be ticked line by line. |

These three files are **committed on purpose** — `.gitignore` excludes the rest of `.deploy/` and negates them specifically. They *are* the deliverable: a reviewer opening the repository has to find the transaction hashes without running anything, and a deliverable that only exists on the machine that produced it is not evidence of anything.

### Three kinds of row, and the difference is the point

| Row | Means |
|---|---|
| a hash, no code | The call succeeded. |
| a hash **and** a code in bold | The spend was **declined and recorded**. A decline is a successful transaction here — that is what lets you open it and read the reason. |
| `simulation` | The contract refused and the call never reached the ledger. Labelled rather than given a hash, because a fabricated hash is the one artefact a reviewer would have no way to catch. |

### How a Chapter Lead checks it

1. Open `evidence.md`. Every hash is a link.
2. Click any of them. Stellar Expert shows the transaction succeeded.
3. Click row 14 — a decline. It succeeded, and the record says `CardFrozen`.
4. Click row 23 — the settlement. 2,400 USDC left the contract.
5. Open `reconciliation.md`. Row `sp_1` links that same transaction to the spend the SaaS card authorised, in whole USDC.
6. Note the line: *"2 of 2 settlements confirmed against the chain."* That is the indexer having fetched each transaction and checked it, not having taken the contract's word.

No code needs to be read, and nothing needs to be run.

## Verifying somebody else's deployment

Everything above can be checked against a treasury you did not deploy:

```bash
cd backend
cargo run --release -- report --contract C… --network testnet
```

Which rebuilds its entire state from events, confirms every settlement against the chain, and prints the table. That is what makes the evidence checkable rather than merely presented.

## Mainnet

**There is none, and there will not be one in Phase 1.**

Mainnet deployment is out of scope per SOW §4.1 and deferred until after a security review that has not happened. Every contract address in this package is a testnet address, and the token in the default path is a fixture anyone can mint.

Stated here, in the README, and in the deck, because it is the single thing most likely to be misread when somebody sees a working demo.
