# 04 · Cards and limits

## Balance and limit are two independent constraints

The deck says it three times, and it is still the thing most likely to get flattened in implementation or in a UI.

```
balance    money that is actually here and assigned to this card
limit      a ceiling on how much may be spent per window,
           regardless of how much money is here
```

Neither number explains the other. A card can be:

- **rich and capped** — $61,000 of balance, $13,000 left of a $50,000 monthly limit. It approves $12,000 and declines $18,900, and the balance had nothing to do with it.
- **broke and free** — $10 of balance against a $90,000 limit. It declines $100, and the limit had nothing to do with it.

The contract keeps them as separate fields, checks them as separate gates, and returns **separate reason codes** — `LimitExceeded` versus `InsufficientCardBalance`. A console that showed one combined bar would leave somebody raising a limit on a card that is out of money, or moving money onto a card that is out of limit. The deck's Cards Grid wireframe draws two bars per card for exactly this reason.

## The card

```rust
struct Card {
    id: u32,
    category: Category,        // SaaS · AI · Advertising · Cloud · Other
    label: String,             // free text, ≤ 32 bytes
    balance: i128,             // money assigned to this card
    limit: i128,               // ceiling per window. 0 is legal
    limit_window_secs: u64,    // the period. 30 days by default
    window_start: u64,         // when the current window opened
    spent_in_window: i128,     // approved spending inside it
    status: CardStatus,        // Active | Frozen
}
```

### Category is an enum, not free text

The deck is emphatic (slide 12), and the reason is downstream rather than aesthetic: a report that groups spending by cost category is only clean if the category is drawn from a closed set. Free text gives you `SaaS`, `saas` and `Saas` as three categories in the same report, and no amount of normalisation afterwards recovers which was meant.

`Other` exists so that a real expense never has to be mislabelled into one of the four named ones. A category that does not fit is a category that goes in `Other` and shows up as unclassified — which is information — rather than one that quietly inflates `Cloud`.

`label` is where the free text goes, bounded at 32 bytes. A card is read on every authorization and storage read cost is charged by size.

### Zero limit is a real state

`limit = 0` is legal and means the card may not spend. It is the resting state of a card that exists but has not yet been budgeted.

It declines with **`LimitExceeded`, not `CardFrozen`** — and the distinction matters operationally. Frozen says *somebody turned this card off*; zero limit says *the card is fine and there is no budget*. Reporting them the same way sends somebody to unfreeze a card that was never frozen.

## The limit window — decision #02

The SOW never says whether a spending limit is per-transaction, daily, or monthly. Rather than picking one globally, **the period is a field on the card**, defaulting to 30 days.

Two cards can therefore be budgeted on different cadences — a daily advertising card next to a monthly SaaS card — which is a thing organisations actually want and which a single global period would have made impossible without a migration.

### The reset is lazy

Nothing on Stellar wakes a contract up. A window that reset "on schedule" would need a keeper transaction every period, per card, and its failure mode would be a card silently stuck at last month's spending.

Instead **every call that cares rolls the window first, then evaluates**:

```rust
if now >= window_start + limit_window_secs {
    let whole = (now - window_start) / limit_window_secs;
    window_start += whole * limit_window_secs;
    spent_in_window = 0;
}
```

The window is therefore always correct at the moment it is read, and correct with nobody maintaining it.

### It advances by whole windows, not to `now`

A card untouched for three and a half months resumes on the window boundary it *would* have had if it had been used every month — not on a boundary starting today. That keeps the reset instants stable and predictable rather than sliding forward each time somebody happens to call.

```
window_start ──┬──────────┬──────────┬──────────┬─────── now
               │          │          │          │    ↑
             month 1    month 2    month 3    month 4
                                              │
                              window_start advances to HERE,
                              and half a window remains
```

`limits_tests.rs::a_long_gap_advances_by_whole_windows` asserts exactly this, including that the half-window remainder still counts.

### Reading it without writing it

`get_card` returns what storage holds — an expired window and all. `get_card_live` and `get_remaining_limit` roll it first.

Both exist because they answer different questions. A tool auditing what the chain *says* wants the stored value. A console about to tell somebody how much headroom they have wants the live one, or it shows last period's figure for a window that has already reset. Neither writes; the roll is persisted by the next call that changes anything.

## Changing a limit

```
set_limit(caller, card_id, limit, window_secs)
```

**Raising takes effect immediately.** It does not wait for the next window.

**Lowering below what has already been spent leaves zero available**, and does not wrap into a negative that would read as headroom:

```rust
fn remaining_limit(card) -> i128 {
    (card.limit - card.spent_in_window).max(0)
}
```

**Changing the period does not forgive spending.** `spent_in_window` is untouched. Shortening a window that has already been spent against would otherwise hand the whole limit straight back — turning a tightening into a loosening, which is the opposite of what the person doing it intended.

`window_secs = 0` means *leave the period alone*, not *zero seconds*. Zero seconds would make the limit meaningless by resetting on every call, so it is refused with `InvalidWindow` when passed anywhere it could take effect.

### The period is bounded at both ends

```
0  <  limit_window_secs  <=  MAX_WINDOW_SECS   (ten years)
```

The upper bound is not tidiness. `roll_window` asks whether `now >= window_start + limit_window_secs`, and `window_start` is a Unix timestamp — so a period near the top of a `u64` overflows that sum.

Before the bound existed, `create_card(…, u64::MAX)` was accepted, and from then on **every call that rolled the window trapped**: `authorize_spend`, `set_limit`, `get_card_live`, `get_remaining_limit`. The card could not spend, and its limit could not be corrected either, because correcting it also rolls the window. One mistyped argument bricked a card permanently, with no upgrade path to undo it.

`roll_window` also uses `checked_add` and treats an overflow as *"not expired"*, as a second line of defence — the failure it prevents is a card nobody can read, write or repair.

## Status

`Active ⇄ Frozen`, and freezing is checked **first**, before any arithmetic. A frozen card declines with `CardFrozen` and nothing else is consulted — a policy decision should not be overridden by a calculation.

**Either role may freeze.** An admin freezes as policy; an operator freezes because something is happening right now, and making them wait for an admin is the wrong trade in the single case where speed is the point.

**Freezing is not a lock on the money.** An admin can still deallocate a frozen card's balance back to the pool. Freezing stops *spending*; a frozen card whose funds were also stuck would make freezing a decision nobody wants to take, which would defeat the purpose of having it.

## Allocation, deallocation and rebalancing

| | What it does | Treasury effect |
|---|---|---|
| `allocate` | unallocated pool → card | `total_allocated` rises |
| `deallocate` | card → unallocated pool | `total_allocated` falls |
| `transfer_between_cards` | card → card | **none** |

The third is worth stating explicitly: a rebalance is a move *inside* the allocated pool, so the sum of card balances is unchanged and no solvency question arises. The treasury does not notice, and must not — a rebalance that changed `total_allocated` would make `unallocated` drift on every reorganisation.

`allocate` is the guard everything downstream assumes held. A card can never hold money the treasury does not have, which is why every later balance check means something. The check is `>`, not `>=`: allocating exactly the whole unallocated pool is legal, and an off-by-one there would strand the last unit of every treasury.
