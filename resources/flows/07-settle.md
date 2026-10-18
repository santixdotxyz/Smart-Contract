# 07 · Settle

The only path money leaves by.

```
operator ──▶ settle(caller, spend_id, to, amount)
                  │
      0  require Operator                        ──▶ [3]  Unauthorized
      0  to is not the contract itself           ──▶ [22] InvalidDestination
      0  spend exists                            ──▶ [9]  SpendNotFound
      1  amount > 0                              ──▶ [4]  InvalidAmount
      2  to is whitelisted                       ──▶ [8]  NotWhitelisted
      3  spend.decision == Approved              ──▶ [10] SpendNotApproved
      4  not settled already                     ──▶ [12] AlreadySettled
      5  amount <= spend.amount                  ──▶ [11] AmountExceedsSpend
      6  amount <= card.balance                  ──▶ [7]  InsufficientCardBalance
                  │
      7  card.balance   -= amount        ← the ORIGINATING card
         total_settled  += amount        ← total_allocated does NOT move
         spend.settled   = true
         write Settlement { spend_id, card_id, amount, to, ledger, ts }
         write SpendSettlement[spend_id] = settlement_id
      8  assert I1
      9  emit card · spend (re-emitted, settled) · settle
                  │
     10  token.transfer(contract ──▶ to, amount)     ← SECOND-TO-LAST
     11  bump TTL
```

## Nine gates, not three

The deck names three — operator, whitelist, amount. Six more are needed for those three to mean anything:

| Gate | Why it exists |
|---|---|
| `to` is not the contract | Paying itself is a no-op transfer whose bookkeeping still runs. The amount leaves the card, never returns to `unallocated`, and becomes unreachable. |
| 3 · approved | A declined record exists **because the contract refused it**. Settling one would make the refusal decorative. |
| 4 · not settled | Invariant I3. |
| 6 · card balance | The approval may be several ledgers old and another settlement may have drained the card. This is what makes the contract the final arbiter of the race window. |

## Debit the originating card, never the pool

```
CORRECT                              WRONG (and it passes a treasury-level check)
  saas    43,300 ──▶ 40,900            saas    43,300 ──▶ 43,300
  unalloc 67,500 ──▶ 67,500            unalloc 67,500 ──▶ 65,100
  held   250,000 ──▶ 247,600           held   250,000 ──▶ 247,600
                       ▲ same                                ▲ same
```

The second version is wrong the moment anyone asks what SaaS has spent this month. The card balance is the record of what a category has left; settling from anywhere else leaves every category report wrong while the headline number looks right.

## Settling for less is allowed

```
spend.amount 2,400
   settle 2,400  ──▶ ✓
   settle 2,000  ──▶ ✓   and the spend CLOSES at 2,000
   settle 2,401  ──▶ [11] AmountExceedsSpend
```

A real settlement often is less — a partial capture, a discount applied after authorization. Requiring exactness would force an operator to choose between over-paying and leaving the spend permanently open.

There is **no partial state to chase**. After `settle`, `settled` is true and the remainder is simply not owed.

## Exactly once — two mechanisms

```
spend.settled = true                       the flag
SpendSettlement[spend_id] = settlement_id  the index — a KEY, so no scan
```

The index's existence is what makes a second settle refusable without walking the settlement ledger. There is no scan, no window, and no way for two settlements of one spend to both pass their checks.

It is also the reverse lookup a reviewer follows:

```
get_settlement_of_spend(spend_id)  ──▶  "what paid this?"
```

## `total_allocated` does not move

The one that looks wrong and is not.

```
Σ card.balance = total_allocated − total_settled

settle 2,400:
   Σ card.balance  drops by 2,400   (the card was debited)
   total_settled   rises by 2,400   ✓ the identity holds
   total_allocated unchanged
```

Decrementing `total_allocated` as well would drop the right-hand side by twice the amount, and `unallocated` would silently grow by every settlement — money appearing from nowhere.

## The transfer is second-to-last

```
… every write lands … emit events … token.transfer … bump TTL
                                          ▲
                          a reentrant token sees a state in which
                          this spend is ALREADY settled, so a
                          reentrant settle is refused by gate 4
```

## The whitelist

```
admin ──▶ add_whitelist(caller, who)     ──▶ [22] if who is the contract itself
                                         ──▶ [20] if already there
admin ──▶ remove_whitelist(caller, who)  ──▶ [20] if not there
```

**The contract may not be its own destination.** Paying itself is a no-op transfer whose
bookkeeping still runs: the card is debited and `total_settled` rises, so the amount leaves the
card and never returns to `unallocated`. The tokens sit in the contract, accounted to nobody,
beyond the reach of every entry point. Refused at both doors — where the mistake is made, and
where the money would move.

A no-op is refused rather than silently succeeding — a script that reports "whitelisted" for a call that changed nothing is a script whose output cannot be trusted as evidence.

**Removing an address does not touch settlements already made to it.** A reconciliation record is a statement about what happened; rewriting history to match current policy would make the ledger useless as evidence, and the first thing anyone would ask on seeing an edited ledger is what else had been edited.

The whitelist is also what bounds a compromised **operator** key: an operator can move money, but only to somewhere an *admin* already approved.
