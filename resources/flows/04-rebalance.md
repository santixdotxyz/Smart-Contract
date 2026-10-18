# 04 · Rebalance

```
admin ──▶ transfer_between_cards(caller, from_id, to_id, amount)
                │
      1. require Admin                    ──▶ [3]  Unauthorized
      2. amount > 0                       ──▶ [4]  InvalidAmount
      3. from_id != to_id                 ──▶ [16] SameCard
      4. both cards exist                 ──▶ [5]  CardNotFound
      5. amount <= from.balance           ──▶ [7]  InsufficientCardBalance
                │
      6. from.balance -= amount
         to.balance   += amount
      7. emit card ×2 { xfer_out, xfer_in }
      8. bump TTL
```

## The treasury does not notice, and must not

```
        ┌──────────────── allocated pool ────────────────┐
        │                                                 │
        │   ┌────────┐   1,000    ┌────────┐             │
        │   │   AI   │ ─────────▶ │  SaaS  │             │
        │   │ 61,000 │            │ 42,300 │             │
        │   └────────┘            └────────┘             │
        │      60,000               43,300                │
        └─────────────────────────────────────────────────┘

total_allocated   unchanged
unallocated       unchanged
Σ card.balance    unchanged
held              unchanged
```

A rebalance is a move **inside** the allocated pool. The sum of card balances does not change, so no solvency question arises and nothing in the treasury moves.

A rebalance that changed `total_allocated` would make `unallocated` drift on every reorganisation — money appearing or disappearing because somebody moved a budget between two categories.

## `SameCard` is refused, not ignored

```
transfer_between_cards(admin, 1, 1, 100)  ──▶ [16] SameCard
```

A self-transfer is a no-op, and a no-op that reports success is a line in a script whose output cannot be trusted. It is also a plausible typo — `from_id` and `to_id` sit next to each other — and this is the cheapest place to catch it.

## What it does not do

**It does not move limits.** Each card keeps its own `limit`, `limit_window_secs`, `window_start` and `spent_in_window`. Moving money onto a card that has exhausted its limit gives it money it still cannot spend — which is correct, and is the case that makes the balance/limit separation visible.

**It does not touch status.** Money can be moved onto or off a frozen card.
