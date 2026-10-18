# 09 · Off-chain integration

Two consumers read this contract. One is shipped in this repository; the other is Phase 2's, and this document is its handover.

| | Shipped? | What it does |
|---|---|---|
| **The indexer** — module M16 | yes, `backend/` | Rebuilds state from events, confirms settlements against the chain, renders the reconciliation report. |
| **The authorization gateway** | no — out of scope | Answers the card network in milliseconds, then writes the decision back. |

### `backend/` is a verification tool, not the application layer SOW §4.1 excludes

Worth stating in the one place a reviewer will look, because the directory name invites the wrong
reading. What lives in `backend/` is a **command-line tool that reads**:

| | |
|---|---|
| Does it serve requests? | No. No HTTP server, no port, no async runtime — look at its four dependencies. |
| Does it hold state? | No database. `--out` writes a JSON snapshot, which is a build artefact regenerated on every run, not a store of record. |
| Does it sign or move money? | No. It holds no key and calls no entry point. It reads RPC. |
| Can it be wrong in a way that matters? | Only by *reporting* wrongly. It has no authority over anything. |

The backend application layer §4.1 excludes is the thing that would sit between a card network and
this contract, holding a cache and signing `authorize_spend` — the **gateway**, which is Phase 2's
and is not built. Part 1 above is its handover.

This tool is module M16, the RPC integration the SOW names by name and funds at 16% of the budget.
It exists so that the evidence package can be checked by a second implementation rather than taken
on the contract's word.

---

## Part 1 · The gateway (Phase 2)

Not built here. SOW §4.1 puts the backend application layer out of scope. What *is* in scope is the set of obligations the contract hands it, and this is that list.

### The shape

```
card network ──► gateway ──► answer in ≤ 2s
                    │
                    ├── reads contract state over RPC (cached)
                    │
                    └── writes authorize_spend, ≥ 1 ledger later
```

The gateway is not a proxy to the contract. It is a **second implementation of the same four rules**, running against a cached copy of the state, and everything difficult about the integration comes from that.

### What it must reproduce exactly

1. **The four checks, in order** — status, then remaining limit, then card balance, then treasury backing. The order decides which reason a decline gets when several apply, and if the two systems disagree the customer is told one thing and the ledger says another.
2. **The lazy window roll**, before check 2. A card whose period lapsed must be judged against the period it is actually in.
3. **`remaining_limit` clamped at zero**, so a limit lowered below what was already spent does not read as headroom.
4. **The reason code names**, verbatim.

`smart-contract/src/spend.rs::evaluate` is a pure function — state in, verdict out, touching neither storage nor the ledger. It is shaped that way deliberately: a gateway written in Rust can link against it rather than reimplementing it from prose and getting it subtly different. That is the recommended route.

### What it must own

**The race window.** The contract holds nothing on authorization (decision #01), so two approvals can pass against one card before either settles. The gateway is where the race actually happens and the only party that can decline the second swipe *before* it is made.

It therefore needs its own reservation model over its cached state: subtract approved-but-unsettled amounts from the balance it believes a card has. The contract will not do this for it.

**Orphan approvals.** An approval the contract later refuses to settle — because another settlement drained the card first — is visible as an approved, unsettled `SpendRecord`. The gateway must reconcile those: either re-settle against a card that has been topped up, or void them with the merchant.

**Staleness.** The gateway's cache is at least one ledger behind. It must know how far behind and decline conservatively when it is too far — a cache that silently serves five-minute-old limits will approve spending that no longer exists.

### What it gets for free

- **Every mutation emits a whole struct**, so keeping a cache current is a fold over `getEvents` rather than a poll of every view function.
- **Declines are recorded on chain**, so the gateway's own decision log can be reconciled against the contract's rather than trusted.
- **`ledger_seq` on every record**, so "approved" and "written on chain" can be shown as the two distinct moments they are.

---

## Part 2 · The indexer — module M16

`backend/indexer`, a native Rust binary. This is the RPC integration the SOW names, at 16% of the budget, and it is a *Must*.

### Three commands

```
index    fold the event stream into state, write it as JSON
verify   …then fetch each settlement's transaction and confirm it succeeded
report   …then render the reconciliation table as markdown
```

```bash
saintix-indexer report \
  --contract CDBY…ERTM \
  --network testnet \
  --from-ledger 4310828
```

### `index` never marks anything reconciled

The separation between `index` and `verify` is the point rather than a convenience.

**Reconciled means one specific thing**: the transaction a record claims to live in was fetched from the node and found to have succeeded. If building state could set that flag, a report would show settlements as corroborated merely because the tool had seen the event announcing them — and seeing an event is not corroboration, it is the claim *being* corroborated.

### Why there is no database

There is nothing to keep. The contract is the source of truth and the fold is idempotent, so the honest thing is to re-read and re-derive.

A cached copy would introduce the one failure mode the whole design avoids: a number in a report that no longer matches the chain, with nothing to say which is right.

### The fold

```
last event per id wins
```

That works because the contract emits whole structs rather than deltas — see [08-events-and-errors.md](08-events-and-errors.md). It makes the fold idempotent (replay a page, nothing changes) and order-tolerant (a page out of order is corrected by the next one carrying a later state).

The one collection with no key of its own is `deposits` — a list rather than a map — so it is guarded by the **event id** the node assigns. Not by transaction hash: one transaction can invoke `deposit` more than once through a wrapper contract, and collapsing those would lose a real deposit.

It used to duplicate on every replay, which made *"the fold is idempotent"* false for the one part of the report a reviewer reads as a history. `state.rs::the_fold_is_idempotent` now asserts the whole state — deposits included — is unchanged after three passes over the same events.

After folding, the indexer **checks the books**: the rebuilt cards must sum to `total_allocated − total_settled`. A page lost in transit shows up there and nowhere else, because the treasury totals stay perfectly self-consistent either way.

### Two traps in `getEvents`, both silent

**An empty page does not mean the end.** `getEvents` does not scan the whole retention window in one call — it walks forward over a bounded span of ledgers, returns whatever it found, and hands back a cursor at the point it stopped. A contract deployed five minutes ago, read from the node's oldest retained ledger, gives an **empty first page** and a cursor a few thousand ledgers along.

A loop that treats "no events" as "no more events" reports a treasury with nothing in it, exits zero, and writes a perfectly well-formed state file full of zeroes. That happened during development of this tool, and it is why the loop follows the cursor until the cursor reaches the node's latest ledger rather than stopping on a short page.

It is also why `scripts/demo.sh` **asserts the rebuild against the chain** rather than trusting the indexer's exit code:

```bash
expect_eq "settlements the indexer rebuilt"      "$(jq '.settlements | length' "$IDX")" "2"
expect_eq "settlements it confirmed"             "$(jq '[…|select(.reconciled)]|length')" "2"
expect_eq "spend records the indexer rebuilt"    "$(jq '.spends | length' "$IDX")"      "6"
expect_eq "total_settled, rebuilt from events"   "$(jq -r '.treasury.total_settled')"   "…"
```

Those five lines compare two **independent** views of the same treasury — what the contract's view functions say, and what the indexer folded out of events. Agreement between them is the whole claim module M13 makes, and without the assertions a paging bug looks exactly like a successful reconciliation.

**Start from the deployment.** `deploy.sh` records the ledger it deployed in and `demo.sh` passes it as `--from-ledger`. Without it the indexer starts at the node's oldest retained ledger and walks forward through a hundred thousand ledgers of unrelated traffic to find a contract deployed a minute ago. Correct either way; seconds versus minutes.

### Decoding

Event topics and bodies arrive as base64 XDR. The indexer decodes them itself rather than asking the node for JSON, because that server-side convenience has changed shape between RPC releases and is not part of any interface the contract controls.

Four shapes are all it needs:

| Rust | ScVal |
|---|---|
| `struct` with named fields | `Map`, keyed by field name as a `Symbol` |
| unit-variant `enum` | `Vec` of one `Symbol` |
| `Option::None` | `Void` |
| `i128` | a hi/lo pair, reassembled |

`i128` is the one worth care: `hi as i128 * 2^64 + lo` overflows for large negatives, and swapping the words produces a number that looks like a balance and is not one. `scval.rs` has a round-trip test across the word boundary including `i128::MIN` and `i128::MAX`.

Fields are looked up **by name**. If a field is renamed in the contract, the decoder fails at that point and says which fields *were* present — rather than silently reading a zero into a reconciliation table.

### Rendering

The report is written for a reader, not a program. SOW §6 sets the bar:

- amounts in **whole USDC** with thousands separators, never smallest units. A reviewer comparing a figure against an invoice must not be off by seven orders of magnitude.
- the formatting is **string arithmetic, not a float**. `250,000.0000001` has more significant digits than an `f64` carries, and a number that is nearly right is worse in a reconciliation table than one that is obviously wrong.
- every hash is a **link** to Stellar Expert. A hash a reviewer cannot click is a hash they will not check.
- **balance and limit get separate columns**, because a card can be rich and capped or broke and free, and one combined figure hides whichever is about to decline the next spend.
- declined rows are labelled as **successful transactions**, with a note saying so, because that is the part a reader will otherwise misread.
- the invariants are **stated**, holding or broken, because "the books balance" is the one claim a reviewer cannot verify by opening a transaction.

### Testing

Every test in `backend/` builds its `ScVal`s in the test itself, so the suite runs with no RPC reachable and gives the same answer on every machine.

What that cannot test is the node's wire format. `scripts/demo.sh` covers that end: it runs this binary against the treasury it has just driven, and a decoder disagreeing with the real RPC fails there. Both halves are needed; neither is sufficient.
