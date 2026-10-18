# 09 · Money map

Where every unit of USDC can be, and what moves it between those places.

```
                        ┌───────────────────────────┐
    depositor ─────────▶│                           │
              deposit   │                           │
                        │    unallocated pool       │
                        │  deposited − allocated    │
                        │                           │
                        └────┬─────────────────▲────┘
                             │                 │
                    allocate │                 │ deallocate
                        [6]  │                 │  [7]
                             ▼                 │
        ┌────────────────────────────────────────────────────┐
        │                                                     │
        │   ┌──────┐   ┌──────┐   ┌──────┐   ┌──────┐        │
        │   │ SaaS │◀─▶│  AI  │◀─▶│ Ads  │◀─▶│Cloud │        │
        │   └──┬───┘   └──────┘   └──────┘   └──────┘        │
        │      │      transfer_between_cards  [7] [16]        │
        │      │                                              │
        └──────┼──────────────────────────────────────────────┘
               │
        settle │  [3] [7] [8] [10] [11] [12]
               ▼
        ┌──────────────────────┐
        │ settlement address   │   whitelisted, admin-approved
        │  OUTSIDE the contract│   what happens next is out of scope
        └──────────────────────┘
```

## Three places, and only three

| Place | Counted by | Who can move it out |
|---|---|---|
| the unallocated pool | `total_deposited − total_allocated` | admin, via `allocate` |
| a card | `card.balance` | admin, via `deallocate` or `transfer_between_cards`; operator, via `settle` |
| gone | `total_settled` | nobody. It has left |

**`authorize_spend` appears nowhere on this map.** It moves no money — it decides and records. That is the whole shape of the two-phase design.

## The identity, in the map's terms

```
Σ card.balance    +    unallocated    =    total_deposited − total_settled
      │                     │                          │
   the cards            the pool                 what the contract
                                                 should be holding
```

And what it **actually** holds is a fourth number only the token can answer:

```
token.balance(contract)
```

The two agree unless an issuer clawed tokens back — which no bug on our side causes, and which surfaces as a declined spend rather than a panic. See [06-authorize-spend.md](06-authorize-spend.md) check 4.

## Which counter each operation moves

| | `deposited` | `allocated` | `settled` | `card.balance` | token moves |
|---|---|---|---|---|---|
| `deposit` | **+** | — | — | — | in |
| `allocate` | — | **+** | — | **+** | no |
| `deallocate` | — | **−** | — | **−** | no |
| `transfer_between_cards` | — | — | — | **−** / **+** | no |
| `authorize_spend` | — | — | — | — | no |
| `settle` | — | **—** | **+** | **−** | **out** |

The two cells worth staring at are in the `settle` row: `total_allocated` does **not** move, and that is what keeps `Σ card.balance = total_allocated − total_settled` true. See [07-settle.md](07-settle.md).

## Only one arrow leaves the contract

```
settle ──▶ a whitelisted address
```

That is the entire attack surface for getting money out, and it is guarded by nine gates. Everything else on this map is internal bookkeeping — money changing which pocket it is in, never which building.

There is no `withdraw_all`, no admin escape hatch, and no upgrade path that could add one.

## The worked example

The deck's own figures, driven end to end by `make demo`.

```
                              pool      cards        settled      held
deposit 250,000            250,000          0             0   250,000
allocate 42.3k saas        207,700     42,300             0   250,000
allocate 61k ai            146,700    103,300             0   250,000
allocate 58.4k ads          88,300    161,700             0   250,000
allocate 25k cloud          63,300    186,700             0   250,000
deallocate 4.2k ads         67,500    182,500             0   250,000
rebalance 1k ai→saas        67,500    182,500             0   250,000   ← nothing
authorize 2,400 saas        67,500    182,500             0   250,000   ← nothing
settle spend 1 · 2,400      67,500    180,100         2,400   247,600   ← money out
```

Check the last row: `180,100 + 67,500 = 247,600 = 250,000 − 2,400`. ✓
