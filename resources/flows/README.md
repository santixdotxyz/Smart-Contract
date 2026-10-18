# Flows

One page per business operation, drawn from the shipped code. Each shows the gates **in execution order** and the exact code every refusal returns.

These are not a specification. [`docs/`](../../docs/README.md) is the specification; this is the shape of an operation when you need to see it rather than read it. Where the two disagree, the docs are right and one of these is stale — say so.

| | Flow | Entry point |
|---|---|---|
| 00 | [Lifecycle](00-lifecycle.md) | the whole thing, once |
| 01 | [Construct](01-construct.md) | `__constructor` · `initialize` |
| 02 | [Deposit](02-deposit.md) | `deposit` |
| 03 | [Allocate](03-allocate.md) | `allocate` · `deallocate` |
| 04 | [Rebalance](04-rebalance.md) | `transfer_between_cards` |
| 05 | [Limits](05-limits.md) | `set_limit`, and the window roll |
| 06 | [Authorize a spend](06-authorize-spend.md) | `authorize_spend` — **the important one** |
| 07 | [Settle](07-settle.md) | `settle` |
| 08 | [Reconcile](08-reconcile.md) | `backend/indexer verify` |
| 09 | [Money map](09-money-map.md) | where every unit can be, and what moves it |

## Reading the diagrams

```
──▶   money or control moves
- -▶  a read
[6]   refuses with Error(Contract, #6) — no transaction reaches the ledger
(R)   records a decline — a SUCCESSFUL transaction with a hash
```

The distinction in the last two lines is the one thing to keep hold of. See [08-events-and-errors.md](../../docs/08-events-and-errors.md).
