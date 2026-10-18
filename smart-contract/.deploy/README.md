# Evidence — SOW §6

**Start here. Nothing needs to be installed or run to check any of it.**

Three files, generated against Stellar **testnet** and committed as they came out — `evidence.md`
and `reconciliation.md` by `make demo`, `coverage.txt` by `make coverage`.

Every transaction hash is a link to Stellar Expert, a third-party explorer with no connection to
this repository.

| File | What it is |
|---|---|
| [evidence.md](evidence.md) | One row per step of the end-to-end run: what was attempted, the error or reason code, and the transaction. **This is the file §6.2 is ticked against.** |
| [reconciliation.md](reconciliation.md) | The same treasury rebuilt **independently** from its event stream by `backend/indexer`, in whole USDC, with every settlement linked to the spend it paid. |
| [coverage.txt](coverage.txt) | How much of the contract the test suite actually reaches. A passing suite says nothing about what it never ran. |

## Or check all of it with one command

```bash
make verify
```

Reads every hash below, asks a Stellar node whether it succeeded, and rebuilds the treasury from
its events. No key, deploys nothing, changes nothing.

## How to check it by hand in two minutes

1. Open **[evidence.md](evidence.md)**. Click any hash — Stellar Expert shows the transaction succeeded.
2. Click **row 14**. It is a *declined* spend, and it **succeeded as a transaction**. That is the design: a refusal that reverted would leave nothing for you to open. The record says `CardFrozen`.
3. Click **row 23**. This is the one where money leaves the contract — 2,400 USDC to the settlement address.
4. Open **[reconciliation.md](reconciliation.md)**. Row `sp_1` links that same transaction back to the spend the SaaS card authorised, so the payment is attributable to a category rather than being a bare transfer.
5. Note the line *"2 of 2 settlements confirmed against the chain."* That is the indexer having **fetched each transaction and checked it**, not having taken the contract's word.

## Three kinds of row, and the difference is the point

| Row in `evidence.md` | Means |
|---|---|
| a hash, no code | The call succeeded. |
| a hash **and** a code in bold | The spend was **declined and recorded**. A decline is a successful transaction here — that is what lets you open it and read why. |
| `simulation` | The contract refused and the call never reached the ledger. The CLI simulates before submitting, so no transaction exists. Labelled rather than given a hash, **because a fabricated hash is the one artefact you would have no way to catch.** |

Every refusal in the run was matched against its **exact** error or reason code. A step that
failed for the *wrong* reason stops the script and produces no file at all — so this file
existing is itself part of the claim.

## Reproducing it, or checking a deployment you were handed

```bash
# rebuild any deployed treasury from its events and confirm every settlement — read-only
cd backend
cargo run --release -- report --contract <CONTRACT_ID> --network testnet
```

To regenerate the whole package from scratch, see **Quickstart** and **Providing the signing key**
in the [root README](../../README.md).

## Scope

Testnet only. **Phase 1 has no mainnet deployment** — it is deferred until after a security review
that has not happened. The token in the default path is a fixture anyone can mint, so no figure
here represents real value.
