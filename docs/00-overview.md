# 00 · Overview

## The problem, in the SOW's own terms

The deck (slide 02) states two problems and is explicit that they are not equally hard.

**Problem one.** A company holds stablecoin in treasury, and its operating costs — SaaS subscriptions, AI tools, advertising, cloud — are billed in fiat. This needs a payment bridge. It is a real problem and it is *someone else's*: card issuers and fiat off-ramps exist, and integrating one is out of scope here.

**Problem two.** The company has no way to allocate, trace and control spending across cost categories. This is not a bridge problem. It is a **state machine for money** — how much each category has, how much it may spend in a period, whether it is currently allowed to spend at all, and what happened to every request. That state machine is what Phase 1 puts on chain.

The deck says it plainly and it is worth repeating, because it determines what belongs in this contract and what does not:

> Problem 02 is what creates the product. Problem 01 needs a payment bridge; problem 02 demands a state machine of money — and that is the part being put on chain in Phase 1.

## What the contract is

**One contract instance per organisation.** It holds that organisation's USDC and nothing else, and knows nothing about any other organisation. Multi-organisation tenancy is out of scope, so there is no registry, no org id, and no shared state — two treasuries deployed from the same wasm are entirely separate.

Inside one instance:

- a **treasury**: everything deposited, everything currently assigned to a card, everything paid out;
- a set of **cards**, one per cost category, each with its own balance, spending limit, limit window and status;
- a **spend ledger** recording every authorization decision, approved and declined alike, with the reason;
- a **settlement ledger** linking each payment out to the spend that caused it;
- a **whitelist** of addresses money may be sent to.

## The architectural claim everything follows from

**The contract is a rule book and a ledger. It is not a payment gateway.**

A card network wants an authorization answer in about two seconds. A Stellar ledger closes in about five. The contract therefore cannot sit in the path of a card swipe, and does not try to:

```
off-chain, milliseconds          the gateway reads the contract's state over RPC,
                                 applies the same four rules, answers the network

on-chain, ≥ 1 ledger later       the contract is where those rules are DEFINED
                                 and where the decision is RECORDED
```

Two consequences run through the whole design:

1. **`authorize_spend` moves no money.** It decides and records. The card is debited later, by `settle`, when a payment has actually been made and there is somewhere to send funds.
2. **There is a race window**, and it is accepted rather than engineered away. Two approvals can pass against one card before either settles. The contract is the final arbiter — the first settlement succeeds, the second is refused for want of balance — and the gateway owns the orphan approval. See [07-invariants-and-security.md](07-invariants-and-security.md).

## Scope boundary

### In scope — the three deliverables

| | Deliverable | What it is |
|---|---|---|
| **D1** | On-Chain Treasury | A treasury contract per organisation: holds stablecoin, accepts deposits, reports available balance and deposit history. |
| **D2** | Category Cards & Spending Control | Split the treasury into cards by category, each with balance, limit and status. A validation entry point that checks and records approved/declined on chain. |
| **D3** | Withdrawal & Reconciliation | The operator debits the originating card, transfers USDC to a whitelisted settlement address, and writes a reconciliation record linking the settlement to the spend. |

### Out of scope — SOW §4.1, *explicitly not included*

Frontend and backend application layer · card issuer/processor integration · third-party stablecoin→fiat conversion · security audit · KYC/AML and compliance workflow · multi-organisation tenancy · price oracle · **mainnet deployment**.

### The contradiction in the SOW, and how it is resolved

§4.1 says the frontend is out of scope. §6 says the evidence must be reviewable by a Chapter Lead **with minimal technical expertise**. Both cannot be satisfied by a product UI, and both must be satisfied.

The deck proposes a thin demo console (its module M18, a *Should*). We do not build one. Instead the evidence is:

- **`make demo`** — one command, on a clean machine, that drives the entire workflow on testnet and writes `smart-contract/.deploy/evidence.md`: one row per step, each with a transaction hash linked to Stellar Expert;
- **`smart-contract/.deploy/reconciliation.md`** — the same treasury rebuilt independently from its event stream by `backend/indexer`, rendered as a table in whole USDC with every settlement linked to the spend it closed;
- **`smart-contract/.deploy/coverage.txt`** — how much of the contract the suite actually reaches;
- a narrated video walking through the above.

This is the deck's own stated fallback when M18 is cut, and it clears the §6 bar without building anything §4.1 excludes: a reviewer opens a row, clicks a hash, and sees the transaction. See [11-deployment.md](11-deployment.md).

## The locked decisions

The deck's slide 25 lists nine things it deliberately leaves open. Leaving them open through implementation means deciding them mid-code, repeatedly and inconsistently. All nine are resolved here and argued in [13-deck-conformance.md](13-deck-conformance.md) §B.

| # | Question | Resolution |
|---|---|---|
| 01 | Authorization race window | No on-chain hold or reserve. The contract is the final arbiter; the gateway handles orphan approvals. Recorded as an accepted risk. |
| 02 | Limit window | A per-card rolling window with an explicit period, 30 days by default, reset **lazily** on read. No keeper. |
| 03 | Authorization vs capture | Not modelled. `Decision` is an enum and `settled` is a flag, so `Captured` can be added in Phase 2 without a storage migration. |
| 04 | Recording declines | **Both** approved and declined spends are written to persistent storage. |
| 05 | Number representation | `i128`, smallest unit, 7 decimals for USDC. `overflow-checks = true` in the release profile. |
| 06 | Operator key management | A hot key, accepted for Phase 1, recorded as technical debt for the deferred security review. |
| 07 | One contract per organisation | No factory. `scripts/deploy.sh` deploys one instance per org. |
| 08 | TTL and archival | Bump TTL on every touched key **and** put the durable history in events; the indexer rebuilds state from the stream alone. |
| 09 | Where the demo console counts | Not built. Evidence is `make demo` plus the indexer's report, both under *Deploy + Demo*. |

## The nineteen modules

The deck's release plan (slides 19–20). All nineteen are shipped.

| Module | Where | Deliverable |
|---|---|---|
| M1 `core::config` | `smart-contract/src/lib.rs` · `config` | D1 |
| M2 `core::roles` | `smart-contract/src/roles.rs` | D1 |
| M3 `treasury::deposit` | `smart-contract/src/lib.rs` | D1 |
| M4 `treasury::accounting` | `smart-contract/src/invariant.rs` | D1 |
| M5 `cards::registry` | `smart-contract/src/lib.rs` | D2 |
| M6 `cards::allocation` | `smart-contract/src/lib.rs` | D2 |
| M7 `cards::limits` | `smart-contract/src/cards.rs` | D2 |
| M8 `cards::status` | `smart-contract/src/lib.rs` | D2 |
| M9 `spend::authorize` | `smart-contract/src/spend.rs` | D2 |
| M10 `settle::whitelist` | `smart-contract/src/lib.rs` | D3 |
| M11 `settle::withdraw` | `smart-contract/src/settle.rs` | D3 |
| M12 `settle::reconciliation` | `smart-contract/src/settle.rs` | D3 |
| M13 `events` | `smart-contract/src/events.rs` | all |
| M14 `tests::unit` | `smart-contract/src/test/` | all |
| M15 `tests::integration` | `smart-contract/src/test/acceptance_tests.rs` | all |
| M16 `rpc::indexer` | `backend/indexer/` | D1 · D3 |
| M17 `deploy::scripts` | `smart-contract/scripts/` | all |
| M18 `demo::console` | **not built** — see the §6 resolution above | — |
| M19 `docs::evidence` | `docs/` · `README.md` · `smart-contract/.deploy/evidence.md` | all |

## Repository layout

```
saintix/
  smart-contract/     the Soroban contract, its tests, and the deploy/demo scripts
  backend/           the RPC indexer — module M16
  docs/               this set, English
  resources/flows/    one diagram per business flow, drawn from the shipped code
```

`smart-contract/` and `backend/` are separate Cargo workspaces on purpose: the contract's `[profile.release]` — size optimisation and, critically, `overflow-checks = true` — only takes effect at a workspace root, and merging them would drop it with nothing failing to say so.

## Where to go next

- The shape of the code: [01-architecture.md](01-architecture.md)
- What each entry point does: [02-contract-interface.md](02-contract-interface.md)
- The frozen error and event tables: [08-events-and-errors.md](08-events-and-errors.md)
- Whether it meets the deck: [13-deck-conformance.md](13-deck-conformance.md)
