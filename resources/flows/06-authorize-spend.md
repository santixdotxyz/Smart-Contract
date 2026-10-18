# 06 · Authorize a spend

The important one.

```
operator ──▶ authorize_spend(caller, card_id, amount, merchant_ref)
                  │
      ┌───────────┴─── these REVERT. No transaction, nothing recorded ───────┐
      │  require Operator                              ──▶ [3]  Unauthorized  │
      │  amount > 0                                    ──▶ [4]  InvalidAmount │
      │  card exists                                   ──▶ [5]  CardNotFound  │
      │  merchant_ref present, ≤ 64 bytes              ──▶ [19] InvalidRef    │
      └────────────────────────────────────────────────────────────────────────┘
                  │
      roll the window if expired
                  │
      read token.balance(contract)   - - ▶  the TOKEN, not our books
                  │
      ┌───────────┴─── these RECORD. A successful transaction, with a hash ───┐
      │  1  status == Active         ──▶ (R) CardFrozen                        │
      │  2  amount <= remaining      ──▶ (R) LimitExceeded                     │
      │  3  amount <= card.balance   ──▶ (R) InsufficientCardBalance           │
      │  4  contract holds enough    ──▶ (R) TreasuryInsolvent                 │
      └────────────────────────────────────────────────────────────────────────┘
                  │ all four pass
      card.spent_in_window += amount           ← the ONLY state change
                  │                              card.balance is NOT touched
      write SpendRecord { decision, reason_code, ledger_seq, timestamp }
      emit spend, and card if it changed
```

## A decline is a success

```
authorize_spend on a frozen card
    ──▶ Ok(SpendRecord { decision: Declined, reason_code: CardFrozen })
    ──▶ a real transaction, with a real hash, that a reviewer can open
```

A reverted transaction leaves nothing to open, and SOW §6 asks a reviewer with minimal technical expertise to read *why* a spend was refused. The four reverting cases above are only the ones where there is nothing sensible to record.

## The order is the deliverable, not the checks

Two conditions usually fail together — a frozen card is generally also over its limit, because that is why somebody froze it. **Which reason comes back is decided entirely by the order.**

```
frozen + over limit + over balance + insolvent   ──▶ CardFrozen
         over limit + over balance + insolvent   ──▶ LimitExceeded
                      over balance + insolvent   ──▶ InsufficientCardBalance
                                     insolvent   ──▶ TreasuryInsolvent
```

The off-chain gateway applies the same four rules to its own cached copy. If the two disagree about the order they give **different reasons for the same declined transaction**, and the reconciliation table stops explaining anything.

## Check 4 reads the token, not the books

```
              I1 guarantees:  treasury covers every card
                                       │
              so check 4 against OUR BOOKS could never fire
                                       │
              it reads token.balance(contract) instead
                                       │
              reachable when tokens leave without us knowing
                                       │
                          ── an issuer CLAWBACK ──
```

The books cannot see a clawback, and the books are exactly what would be wrong if they could. The shortfall becomes a recorded decline rather than a panic, so a treasury that is merely *short* does not become one that is *frozen*.

## Nothing moves

```
before      card.balance 42,300      spent_in_window     0     held 250,000
authorize 2,400 → Approved
after       card.balance 42,300      spent_in_window 2,400     held 250,000
                     ▲                        ▲
              UNCHANGED                the only change
```

An authorization is a **promise**, and the promise may never be kept — a merchant may not capture, the gateway may reverse it. Debiting here would leave the card short for a payment that never happened, with no state to say so.

The card is debited by [`settle`](07-settle.md).

## The race window

Because nothing is held, two authorizations against one card both pass check 3.

```
card balance 60

   authorize 40  ──▶ Approved      balance still 60
   authorize 40  ──▶ Approved      balance still 60

   settle #1 for 40  ──▶ ✓          balance 20
   settle #2 for 40  ──▶ [7] InsufficientCardBalance
                              │
                    the orphan: an approved, unsettled record.
                    Visible, and the gateway's problem.
```

Accepted rather than engineered away (decision #01). A hold needs an expiry, an expiry needs something to run at the deadline, and nothing on Stellar wakes a contract up — so an expiring hold means a keeper per hold, whose failure mode is money stranded on a card for a payment nobody made.

**What the contract guarantees instead is that it never over-pays.** `settle` re-checks, and the second one is refused.
