# 08 · Reconcile

Off-chain. Module M16, `backend/indexer`.

```
                 Soroban RPC
                      │
       getEvents ─────┤────── getTransaction
            │         │              │
            ▼         │              ▼
     ┌────────────┐   │       ┌─────────────┐
     │  the FOLD  │   │       │   confirm   │
     │            │   │       │ each tx     │
     │ last event │   │       │ SUCCEEDED   │
     │ per id wins│   │       └──────┬──────┘
     └─────┬──────┘   │              │
           │          │              ▼
           ▼          │        reconciled = true
      rebuilt state   │
           │          │
           ▼          ▼
      check the books · render the table
```

## Three commands, and the separation is the point

```
index    fold the event stream into state
verify   …then fetch each settlement's transaction and confirm it succeeded
report   …then render the reconciliation table as markdown
```

**`index` never marks anything reconciled.** Reconciled means one specific thing — the transaction a record claims to live in was fetched from the node and found to have succeeded. If building state could set that flag, a report would show settlements as corroborated merely because the tool had seen the event announcing them.

Seeing an event is not corroboration. It is the claim *being* corroborated.

## The fold

```
last event per id wins
```

Works because the contract emits **whole structs rather than deltas**. The last `card` event for id 2 *is* card 2, whatever happened before it.

```
delta stream                          whole-struct stream
  +100  −40  +25                        {balance: 100} {balance: 60} {balance: 85}
  miss one ──▶ silently wrong           miss one ──▶ corrected by the next
  replay one ──▶ silently wrong         replay one ──▶ no change
```

That makes the fold **idempotent** and **order-tolerant**, which is what makes an at-least-once RPC feed safe.

## Then check the books

```
Σ rebuilt card balances  ==  total_allocated − total_settled  ?
```

A page of events lost in transit shows up **there and nowhere else** — the treasury totals arrive as whole structs too, so they stay perfectly self-consistent with each other whatever was missed.

## The trap: an empty page does not mean the end

```
getEvents(startLedger: 4189843)
    ──▶ events: []          cursor: 4192000
                  ▲                     ▲
          NOT the end                the node walked a bounded
                                     span and stopped there
```

`getEvents` does not scan the whole retention window in one call. A contract deployed five minutes ago, read from the node's oldest retained ledger, gives an **empty first page**.

A loop that treats "no events" as "no more events" reports an empty treasury, **exits zero**, and writes a perfectly well-formed state file full of zeroes. That is exactly the kind of result that looks like evidence.

So the loop follows the cursor until the cursor's ledger reaches the node's latest, not until a page comes back short:

```
loop:
    page = getEvents(cursor or startLedger)
    fold(page.events)
    if no cursor:                        break
    if cursor.ledger >= page.latestLedger: break
    if cursor did not move:              break     ← a misbehaving node
```

And `demo.sh` **asserts the result against the chain** rather than trusting the exit code:

```bash
expect_eq "settlements the indexer rebuilt"  "$(jq '.settlements|length' idx)"  "2"
expect_eq "spend records rebuilt"            "$(jq '.spends|length' idx)"       "6"
expect_eq "total_settled from events alone"  "$(jq -r '.treasury.total_settled')" "…"
```

Two **independent** views of the same treasury — the contract's view functions, and the fold over its events. Agreement between them is the whole claim module M13 makes.

## Confirming a settlement

```
for each settlement:
    getTransaction(settlement.tx_hash)
        SUCCESS    ──▶ reconciled = true
        FAILED     ──▶ ERROR. Stop. Write nothing.
        NOT_FOUND  ──▶ report it, do not fail
```

**`FAILED` is fatal.** A settlement record the ledger does not corroborate is the most serious thing this tool can find, and a table with one row quietly marked "not confirmed" is a document that looks like evidence and is not.

**`NOT_FOUND` is not.** A transaction outside the node's retention window is not a fraudulent one, and calling it failed would impugn a perfectly good settlement because the tool was run a month late.

## Settled versus reconciled

| | Set by | Means |
|---|---|---|
| **settled** | the contract, in `settle` | A settlement record was written and a transfer issued |
| **reconciled** | the indexer, in `verify` | **Somebody fetched that transaction and it succeeded** |

The contract cannot mark anything reconciled: it has no way to look at its own transaction after the fact. That is why this step is off-chain, and why the deck putting `Reconciled` in the on-chain spend lifecycle is one of the places it needs a correction.

## Start from the deployment

```
deploy.sh   notes the current ledger BEFORE deploying  ──▶ .deploy/env
demo.sh     passes it as --from-ledger
```

Without it the indexer starts at the node's oldest retained ledger and walks through a hundred thousand ledgers of unrelated traffic to find a contract deployed a minute ago. Correct either way — seconds instead of minutes.
