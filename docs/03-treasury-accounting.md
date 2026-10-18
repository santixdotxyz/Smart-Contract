# 03 · Treasury accounting

## Three numbers, and everything else is derived

```rust
struct Treasury {
    total_deposited: i128,   // everything ever deposited.        Never decreases.
    total_allocated: i128,   // everything currently on a card,
                             //   INCLUDING amounts already settled out of it.
    total_settled:   i128,   // everything ever paid out.         Never decreases.
}
```

Two identities follow, and the whole design rests on them:

```
unallocated       = total_deposited − total_allocated
Σ card.balance    = total_allocated − total_settled
```

Add them:

```
Σ card.balance + unallocated = total_deposited − total_settled
```

which is the deck's stated invariant — *"Σ card.balance + unallocated == the USDC the contract holds"* — with the right-hand side written in terms the contract can check without asking the token.

## Why the totals exist rather than summing the cards

Summing every card on every call would mean loading every card on every call, and that cost grows with the number of categories until an entry point stops fitting in a transaction. The running totals make the check three integer reads.

The cost of that choice is precise and worth stating: **the totals are internally consistent by construction.** If `allocate` raised `total_allocated` and forgot to credit the card, the totals would still agree with each other and the in-contract assertion would pass. Only summing the actual cards catches that, and that is what the test harness's `check_books` does after every scenario — see [10-testing.md](10-testing.md).

## The counter discipline

Which counter each operation moves is the part that is easy to get wrong and quiet when wrong.

| Operation | `total_deposited` | `total_allocated` | `total_settled` | `card.balance` |
|---|---|---|---|---|
| `deposit` | `+ amount` | — | — | — |
| `allocate` | — | `+ amount` | — | `+ amount` |
| `deallocate` | — | `− amount` | — | `− amount` |
| `transfer_between_cards` | — | — | — | `− amount` / `+ amount` |
| `authorize_spend` | — | — | — | **—** |
| `settle` | — | **—** | `+ amount` | `− amount` |

Two rows deserve attention.

**`authorize_spend` moves nothing.** It records a decision. `spent_in_window` rises on approval; the balance does not change until money actually leaves.

**`settle` leaves `total_allocated` alone.** This is the one that looks wrong and is not. The card balance drops by `amount`, so `Σ card.balance` drops by `amount` — and for the identity `Σ card.balance = total_allocated − total_settled` to keep holding, the right-hand side must drop by the same. Incrementing `total_settled` does that. Decrementing `total_allocated` as well would drop it by twice the amount, and `unallocated` would silently grow by every settlement — money appearing from nowhere.

## Decimals and units — decision #05

**`i128`, in the token's smallest unit, everywhere.** USDC on Stellar has 7 decimals, so one whole USDC is `10_000_000`. Nothing in the contract stores a whole-unit figure, converts between scales, or uses a float.

`Config.decimals` is read **from the token contract** at construction, never taken from the deployer. It is not used in any arithmetic — there is none that scales — but it is what an explorer and the indexer render amounts with, and a value copied from an assumption rather than read from the token would be wrong by orders of magnitude with nothing on chain to contradict it.

### There is no arithmetic here worth rounding

That is a property of the design rather than an omission. Saintix moves whole token units between three places — the unallocated pool, a card, and a settlement address — and never multiplies, divides, or converts between assets. There is no price, no interest, no fee, no second decimal scale to reconcile.

So the two things that can still go wrong are:

1. **a smallest unit going missing at a boundary** — a `>=` where the code says `>`, making the last unit of every card and every limit permanently unusable. Invisible at ordinary scale; `decimals_tests.rs` asserts both directions at every ceiling.
2. **a subtraction wrapping instead of trapping.** Every subtraction in the contract is guarded by a check that runs first, so the amount that would underflow is refused with a named error. And `overflow-checks = true` is set in the release profile as the backstop — without it Rust turns overflow checks off in release, and a wrap would happen only in the deployed wasm while every test passed in debug.

## The fourth number: what the contract actually holds

The three counters describe what the contract *believes*. What it **holds** is a question only the token can answer:

```
token.balance(contract_address)
```

In the ordinary case the two agree exactly:

```
token.balance(contract) == total_deposited − total_settled
```

They can come apart, in one direction, without any bug on our side: on Stellar an issuer with clawback enabled can remove tokens from any holder, this contract included. That is why the identity above is a *comparison the contract makes*, not an assertion it panics on — see [07-invariants-and-security.md](07-invariants-and-security.md) for I2 and why `TREASURY_INSOLVENT` is a recorded decline rather than a revert.

## Available balance — deliverable D1's report

The deck's Treasury Overview wireframe shows three figures. They come from:

| Wireframe | Query |
|---|---|
| Total deposited | `get_treasury().total_deposited` |
| Allocated | `get_treasury().total_allocated` |
| Unallocated | `get_unallocated()` |

And the deposit history below them comes from the event stream, not from a stored list — decision #08. Each `deposit` event carries the depositor, the amount, the resulting treasury and (from the RPC envelope) the transaction hash, which is exactly what the wireframe's four columns need. `backend/indexer report` renders it.

## Worked example

The figures from the deck's own wireframes, driven end to end. This is the `demo.sh` scenario; the same numbers are asserted in `demo_replay.rs`.

```
deposit 250,000
  deposited 250,000   allocated       0   settled      0   unallocated 250,000
  held      250,000                                        Σ balances        0

allocate 42,300 saas · 61,000 ai · 58,400 ads · 25,000 cloud
  deposited 250,000   allocated 186,700   settled      0   unallocated  63,300
  held      250,000                                        Σ balances  186,700

deallocate 4,200 from ads
  deposited 250,000   allocated 182,500   settled      0   unallocated  67,500
  held      250,000                                        Σ balances  182,500

rebalance 1,000 ai → saas             ← the treasury does not notice, and must not
  deposited 250,000   allocated 182,500   settled      0   unallocated  67,500
  held      250,000                                        Σ balances  182,500

authorize 2,400 on saas → Approved    ← nothing moves. spent_in_window rises
  deposited 250,000   allocated 182,500   settled      0   unallocated  67,500
  held      250,000                                        Σ balances  182,500

settle spend 1 for 2,400              ← the only step where money leaves
  deposited 250,000   allocated 182,500   settled  2,400   unallocated  67,500
  held      247,600                                        Σ balances  180,100
```

Check the identity on the last line: `180,100 + 67,500 = 247,600 = 250,000 − 2,400`. ✓
