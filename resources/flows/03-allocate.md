# 03 · Allocate and deallocate

```
                    ┌──────────────────────┐
                    │   unallocated pool   │
                    │  deposited − alloc'd │
                    └──────────┬───────────┘
                       allocate │  ▲ deallocate
                                ▼  │
        ┌───────────┬───────────┬──┴────────┬───────────┐
        │   SaaS    │    AI     │    Ads    │   Cloud   │
        │  balance  │  balance  │  balance  │  balance  │
        │  limit    │  limit    │  limit    │  limit    │
        │  status   │  status   │  status   │  status   │
        └───────────┴───────────┴───────────┴───────────┘
```

## `allocate`

```
admin ──▶ allocate(caller, card_id, amount)
                │
      1. require Admin                       ──▶ [3] Unauthorized
      2. amount > 0                          ──▶ [4] InvalidAmount
      3. card exists                         ──▶ [5] CardNotFound
      4. amount <= unallocated               ──▶ [6] OverAllocation   ← THE guard
                │
      5. total_allocated += amount
         card.balance    += amount
      6. assert I1
      7. emit card { action: alloc, card, treasury }
      8. bump TTL
```

**Gate 4 is what everything downstream assumes held.** Every balance check in `authorize_spend` and every debit in `settle` is only meaningful because a card can never hold money the treasury does not have.

The check is `>`, not `>=` — allocating exactly the whole pool is legal. An off-by-one there would strand the last unit of every treasury.

## `deallocate`

The reverse, with gate 4 becoming `amount <= card.balance` → `[7] InsufficientCardBalance`.

```
admin ──▶ deallocate(caller, card_id, amount)
                │
      4. amount <= card.balance              ──▶ [7] InsufficientCardBalance
                │
      5. total_allocated -= amount
         card.balance    -= amount
```

### A frozen card can still be deallocated

```
card.status = Frozen
    deallocate  ──▶ ✓ allowed
    spend       ──▶ (R) declined · CardFrozen
```

Freezing stops **spending**. A frozen card whose funds were also stuck would make freezing a decision nobody wants to take, which would defeat the point of having it.

## `create_card`

```
admin ──▶ create_card(caller, category, label, limit, window_secs)
                │
      1. require Admin                       ──▶ [3] Unauthorized
      2. label present, ≤ 32 bytes           ──▶ [18] InvalidLabel
      3. limit >= 0                          ──▶ [14] InvalidLimit
      4. window_secs == 0 → 30 days
         window_secs > 0                     ──▶ [15] InvalidWindow
                │
      5. Card { next id, category, label, balance: 0,
                limit, window opening now, spent: 0, Active }
      6. emit card { action: create }
```

`category` is an **enum** — `SaaS · AI · Advertising · Cloud · Other` — never a string. A report that groups spending by category is only clean if the category comes from a closed set; free text gives you `SaaS`, `saas` and `Saas` as three categories in one report.

`limit = 0` is legal and means the card may not spend yet. It declines with `LimitExceeded`, **not** `CardFrozen` — the card is fine, the budget is not.

## What the treasury sees

| | `total_deposited` | `total_allocated` | `unallocated` | `Σ balances` |
|---|---|---|---|---|
| after deposit 250,000 | 250,000 | 0 | 250,000 | 0 |
| allocate 42,300 saas | 250,000 | 42,300 | 207,700 | 42,300 |
| allocate 61,000 ai | 250,000 | 103,300 | 146,700 | 103,300 |
| deallocate 4,200 ai | 250,000 | 99,100 | 150,900 | 99,100 |
