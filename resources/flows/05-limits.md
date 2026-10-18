# 05 · Limits and the window

## Two independent constraints

```
   balance   money that is actually here, assigned to this card
   limit     a ceiling on spending per window, whatever the balance is

   ┌─────────────────────────────────────────────────┐
   │  AI Tools                              Active    │
   │                                                  │
   │  balance  61,000  ████████████████░░░░  82%      │
   │  limit    37,000 / 50,000  ██████████████░░  74% │
   │                                                  │
   │  ▶ approves 12,000   ▶ declines 18,900           │
   │    and the balance had nothing to do with either │
   └─────────────────────────────────────────────────┘
```

Neither number explains the other. A card can be **rich and capped**, or **broke and free** — and the two decline with different reason codes because they need different fixes.

## The lazy roll

Nothing on Stellar wakes a contract up. A window that reset on a schedule would need a keeper transaction per card per period, and its failure mode would be a card silently stuck at last month's spending.

So **every call that cares rolls the window first, then evaluates**:

```
roll_window(card):
    now = ledger.timestamp
    # checked_add: an overflow reads as "not expired" rather than trapping
    if now < window_start + limit_window_secs:  return unchanged
    whole        = (now − window_start) / limit_window_secs
    window_start += whole × limit_window_secs
    spent_in_window = 0
```

### Whole windows, not to `now`

```
window_start ─┬──────────┬──────────┬──────────┬──────── now
              │          │          │          │      ↑
            month 1    month 2    month 3    month 4  │
                                              │       │
                            window_start moves HERE   │
                                              └───────┘
                                          half a window remains,
                                          and the limit is still spent
```

A card untouched for three and a half months resumes on the boundary it *would* have had if it had been used every month — so the reset instants stay stable rather than sliding forward each time somebody happens to call.

## Reading it without writing it

```
get_card(id)            what STORAGE holds — expired window and all
get_card_live(id)       the same card with an expired window rolled
get_remaining_limit(id) headroom now
```

Both exist because they answer different questions. A tool auditing what the chain *says* wants the stored value; a console about to tell somebody how much headroom they have wants the live one, or it shows last period's figure for a window that has already reset.

Neither writes. The roll is persisted by the next call that changes anything.

## `set_limit`

```
admin ──▶ set_limit(caller, card_id, limit, window_secs)
                │
      1. require Admin                     ──▶ [3]  Unauthorized
      2. limit >= 0                        ──▶ [14] InvalidLimit
      3. window_secs != 0 ?                ──▶ [15] InvalidWindow unless
                                                  0 < it <= MAX_WINDOW_SECS
      4. card exists                       ──▶ [5]  CardNotFound
                │  ─── everything above validates. nothing has changed yet ───
      5. roll the window if expired
      6. card.limit = limit
         card.limit_window_secs = window_secs   (if one was given)
      7. emit card { action: limit }
```

Steps 1–4 run **before** anything is mutated. The host would roll a rejected call back anyway,
but a function that half-applies its changes and then decides whether to fail is one reordering
away from a real bug.

| Change | Effect |
|---|---|
| raise the limit | **immediate.** Does not wait for the next window |
| lower below what is spent | headroom is `0`, clamped — never a negative that reads as available |
| change the period | `spent_in_window` is **untouched** |
| `window_secs = 0` | means *leave the period alone*, not *zero seconds* |

### Why changing the period does not forgive spending

```
limit 1,000 · spent 900 · monthly
   set_limit(limit: 1,000, window: DAILY)
limit 1,000 · spent 900 · daily        ← still 100 left, not 1,000
```

Shortening a window that has already been spent against would otherwise hand the whole limit straight back — turning a tightening into a loosening, which is the opposite of what the person doing it intended.

## Remaining is clamped

```
remaining = max(limit − spent_in_window, 0)
```

Without the clamp, a limit lowered below what was already spent would produce a negative that arithmetic elsewhere could read as headroom.
