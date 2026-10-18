# Saintix — build documentation

Specification for the Soroban contract layer behind Saintix: an **on-chain treasury** — one contract per organisation — that holds USDC, splits it into category-based virtual cards each with its own balance and spending limit, records every spend authorization decision, and settles approved spends to whitelisted addresses with a reconciliation record linking each payment back to the spend that caused it.

**This documentation covers the contract layer and its off-chain reader.** SOW §4.1 puts the frontend and backend **application layer** explicitly out of scope, and there is none in this repository. What lives here besides the contract is `backend/indexer` — the RPC integration the SOW asks for by name — and the scripts that produce the evidence package.

The directory is called `backend/` and the distinction matters: it holds a **read-only command-line tool** with no server, no database and no keys. The application layer §4.1 excludes is the gateway that would sit between a card network and this contract — Phase 2's, and not built. [09-offchain-integration.md](09-offchain-integration.md) draws the line explicitly.

## Source of truth

Everything here derives from one file:

```
resources/specs/Saintix_SOW1_Deck.html
```

References to *slide N* point into that file, numbered in display order. It is the client's build brief and the **only non-English file here**: it is kept so that [13-deck-conformance.md](13-deck-conformance.md) can be checked against the document it audits.

References to *slide N* point into that file, numbered in display order. Where the deck contradicts itself or leaves something unspecified, this documentation records the resolution and the reasoning behind it — [13-deck-conformance.md](13-deck-conformance.md) is where every such resolution is registered.

**If you are checking whether the contract meets the deck, start at [13-deck-conformance.md](13-deck-conformance.md).** It is the only file organised by the deck rather than by the contract, and it names the test that proves each line.

**If you want the shape of an operation rather than its specification, start at [`resources/flows/`](../resources/flows/README.md)** — one diagram per business flow, drawn from the shipped code, showing the gates in execution order and the exact code every refusal returns.

If you are about to implement something and this set does not answer your question, the answer does not exist yet — add it to [13-deck-conformance.md](13-deck-conformance.md) rather than inventing it in code.

## Reading order

| # | Document | Covers |
|---|---|---|
| 00 | [00-overview.md](00-overview.md) | What the contract is, the scope boundary, the nine locked decisions |
| 01 | [01-architecture.md](01-architecture.md) | Modules, storage layout, durability, the write-order discipline |
| 02 | [02-contract-interface.md](02-contract-interface.md) | Every entry point: signature, auth, gates, state deltas, events |
| 03 | [03-treasury-accounting.md](03-treasury-accounting.md) | The three counters, decimals, checked arithmetic |
| 04 | [04-cards-and-limits.md](04-cards-and-limits.md) | Balance and limit as two independent constraints; the window |
| 05 | [05-spend-authorization.md](05-spend-authorization.md) | The four checks, why the order is frozen, the off-chain split |
| 06 | [06-settlement-and-reconciliation.md](06-settlement-and-reconciliation.md) | The guards, the `spend_id` link, what *Reconciled* means |
| 07 | [07-invariants-and-security.md](07-invariants-and-security.md) | I1–I3, the threat model, the accepted risks |
| 08 | [08-events-and-errors.md](08-events-and-errors.md) | **The frozen tables** — errors, reason codes, the event catalogue |
| 09 | [09-offchain-integration.md](09-offchain-integration.md) | The contract's obligations to the gateway and the indexer |
| 10 | [10-testing.md](10-testing.md) | Test matrix, T1–T10, the acceptance scenario |
| 11 | [11-deployment.md](11-deployment.md) | Build, deploy, testnet, the evidence package |
| 12 | [12-roadmap.md](12-roadmap.md) | What Phase 2 needs and what Phase 1 left open on purpose |
| 13 | [13-deck-conformance.md](13-deck-conformance.md) | **The audit record** — the deck line by line against the shipped contract |

## Shape of the contract in one page

```
Deploy      one contract per ORGANISATION. No factory, no tenancy, no shared state
Roles       admin (shape of the money) · operator (movement of the money). Two, and no super-user
Asset       one SEP-41 token, fixed at construction. USDC on Stellar: 7 decimals
Treasury    three running totals; every other figure is derived from them
Cards       category · balance · limit · rolling window · status. Balance and limit are INDEPENDENT
Spend       four checks in a frozen order; approved AND declined are recorded on chain
Settlement  operator only · whitelisted destination only · at most the spend · exactly once
Governance  no upgrade path, no admin override, no pause. The last admin cannot be revoked
Off-chain   the gateway decides in milliseconds; the contract is the rule book and the ledger
```

## The five decisions that shape the code

All nine of the deck's open questions are resolved in [13-deck-conformance.md](13-deck-conformance.md) §B. These five are visible in almost every file:

| # | Question the deck left open | What we build |
|---|---|---|
| 01 | The authorization race window | **No on-chain hold.** Two approvals can pass against one card before either settles. The contract is the final arbiter — `settle` re-checks and refuses the second — and the gateway owns the orphan approval. |
| 02 | Is the limit per-transaction, daily, or monthly? | **A per-card rolling window**, 30 days by default, reset lazily on read. No keeper transaction, and two cards can be budgeted on different cadences. |
| 04 | Record declines on chain, or only emit them? | **Record them.** SOW §6 wants a reviewer to open a declined transaction and read the reason; an event-only decline gives them nothing to query. |
| 05 | Number representation | `i128` in the token's smallest unit throughout, `overflow-checks` on in release. No floats, no rounding, nothing that scales. |
| 08 | Persistent storage TTL | **Bump on every touched key, and put the durable history in events.** `backend/indexer` rebuilds all state from the stream alone — which is also module M13's definition of done. |

## Where the deck is refined rather than followed

Three places where this specification deliberately differs. Each is argued in full in [13-deck-conformance.md](13-deck-conformance.md) §B.

| Deck says | We build | Why |
|---|---|---|
| A declined spend sits next to `panic` in the flow diagrams | A decline is a **successful transaction** carrying a `SpendRecord` | A panic leaves no transaction. The evidence table needs a declined row a reviewer can open — §B-2 |
| `Σ card.balance + unallocated == token.balance(contract)`, asserted | The **accounting identity** is asserted; the **backing** is consulted | On Stellar an issuer with clawback can take tokens from any holder. Asserting equality would freeze a treasury that is merely short — §B-3 |
| Reason codes as numbers | Reason codes as **names** on chain | `reason_code: 2` fails the "minimal technical expertise" bar. `LimitExceeded` does not — §B-5 |

## What is not here, and will not be

Out of scope per SOW §4.1, and nothing in this repository pretends otherwise: frontend and backend application layer, card issuer or processor integration, stablecoin→fiat conversion, security audit, KYC/AML, multi-organisation tenancy, price oracle.

**And no mainnet deployment.** Phase 1 is testnet only; mainnet is deferred until after a security review that has not happened. Every contract address in the evidence package is a testnet address.
