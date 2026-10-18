# 06 · Settlement and reconciliation

## The only path money leaves by

```
settle(caller, spend_id, to, amount)
```

One entry point, one direction, nine gates. Everything else in the contract moves money *within* the contract; this is the step where a transaction hash on Stellar Expert corresponds to USDC actually arriving somewhere.

It is also Deliverable 3's entire evidence: a settlement transaction plus the `spend_id` on its record is what turns *"this contract sent 2,400 USDC"* into *"this contract paid the AWS invoice that the SaaS card authorised on the 24th"*.

## The gates

The deck names three. Six more are needed for those three to mean anything.

| # | Check | Refuses with | Why it exists |
|---|---|---|---|
| 0 | caller holds `Operator` | `Unauthorized` (3) | deck guard 1 |
| 0 | `to` is not the contract itself | `InvalidDestination` (22) | **see below** |
| 0 | the spend exists | `SpendNotFound` (9) | |
| 1 | `amount > 0` | `InvalidAmount` (4) | |
| 2 | `to` is whitelisted | `NotWhitelisted` (8) | deck guard 2 |
| 3 | `spend.decision == Approved` | `SpendNotApproved` (10) | **see below** |
| 4 | not settled already | `AlreadySettled` (12) | invariant I3 |
| 5 | `amount <= spend.amount` | `AmountExceedsSpend` (11) | deck guard 3 |
| 6 | `amount <= card.balance` | `InsufficientCardBalance` (7) | **see below** |

**Gate 3** is required precisely because declines are recorded (decision #04). A declined `SpendRecord` exists *because the contract refused it*. If it could then be settled, the refusal would have been decorative — the contract would have said no and paid anyway.

**The destination guard** stops the treasury paying itself. That transfer is a no-op, but every write around it still runs: the card is debited and `total_settled` rises, so the amount leaves the card and never returns to `unallocated`. The tokens stay in the contract, accounted to nobody and beyond the reach of every entry point. It is an ordinary mistake to make — a contract id and a settlement address are both Stellar strkeys — so it is refused at `add_whitelist` too.

**Gate 6** is what makes the contract the final arbiter of the race window. The approval that authorised this spend may be several ledgers old, and another settlement may have drained the card since. Re-checking is what guarantees the contract never over-pays. See [05-spend-authorization.md](05-spend-authorization.md).

## Debit the originating card, never the pool

The SOW is explicit and it is the requirement most likely to be got wrong under time pressure — because taking it from `unallocated` is simpler and the top-line total comes out identical.

It is not identical. The card balance is the record of what a category *has left to spend*. Settling from anywhere else leaves every category report wrong while the headline number looks right:

```
correct                                 wrong
  saas   43,300 → 40,900                  saas   43,300 → 43,300
  unallocated 67,500 → 67,500             unallocated 67,500 → 65,100
  held   250,000 → 247,600                held   250,000 → 247,600
             ↑ same                                  ↑ same
```

The second version passes any check that only looks at the treasury. It is wrong the moment anyone asks what SaaS has spent this month.

`deck_conformance.rs::f3_2_settlement_debits_the_originating_card` asserts the card that spent is the card that paid, *and* that the other cards and the pool are untouched.

## Settling for less is allowed

`amount <= spend.amount`, not `==`.

A real settlement often is less — a partial capture, a discount applied after authorization, a merchant billing the actual usage rather than the estimate. Requiring exactness would force an operator to choose between over-paying and leaving the spend permanently open.

The spend closes at whatever figure it closed for. There is **no partial state to chase**: after `settle`, `spend.settled` is true and the remainder is simply not owed. Phase 1 does not model partial capture with a running remainder, and the deck (slide 15) asks that we not build those states — only that `SpendRecord` leaves room to add them.

## Exactly once — invariant I3

Two mechanisms, and both are checked.

**The flag.** `spend.settled` is set to true by `settle`.

**The index.** `SpendSettlement(spend_id) → settlement_id` is written at the same time. Its *existence* is what makes a second settle refusable without scanning the settlement ledger — there is no scan, no window, and no way for two settlements of one spend to both pass their checks.

They cannot disagree unless something is very wrong, and checking both costs one storage read that has already happened. The test harness's `check_books` asserts the two agree for every spend after every scenario.

The index is also the **reverse lookup a reviewer follows**: `get_settlement_of_spend(spend_id)` answers *"what paid this?"*, which is the question somebody holding an invoice actually has.

## The record

```rust
struct Settlement {
    id: u32,
    spend_id: u32,      // THE LINK. Without it, a transfer nobody can account for
    card_id: u32,       // denormalised from the spend
    amount: i128,
    to: Address,
    ledger_seq: u32,
    timestamp: u64,
}
```

`card_id` is copied from the spend rather than looked up. That is deliberate: it lets the reconciliation table be built from settlements alone, which is what `backend/indexer report` does, without a join back through the spend ledger for every row.

## The whitelist

Only `Admin` may change it. Only whitelisted addresses may receive money. That is the whole of it, plus one rule about history:

**Removing an address does not touch settlements already made to it.** A reconciliation record is a statement about what happened. Rewriting history to match current policy would make the ledger useless as evidence — and the first thing anyone would ask on seeing an edited ledger is what else had been edited.

So after a removal, past settlements to that address stand and read exactly as before, and nothing new may go there. `settle_tests.rs::removing_an_address_leaves_past_settlements_alone` asserts both halves.

A no-op change is refused with `WhitelistUnchanged` rather than silently succeeding. A script that reports "whitelisted" for a call that changed nothing is a script whose output cannot be trusted as evidence.

### What the whitelist is for in Phase 1

It is a **placeholder settlement account**, and the SOW says so. There is no fiat conversion, no card issuer, no off-ramp. Money goes to an address an admin approved, and what happens after that is out of scope.

The guard is still the most valuable one in the contract, because it is what bounds the damage from a compromised operator key: an operator can move money, but only to somewhere an *admin* already approved. See [07-invariants-and-security.md](07-invariants-and-security.md).

## Write order

`settle` follows the ten-step discipline, and step 9 is load-bearing here in a way it is not elsewhere:

```
1  require_auth(Operator)
2–5  the nine gates above
6  card.balance −= amount · total_settled += amount · spend.settled = true
7  assert the accounting identity
8  emit card, spend (re-emitted, now settled), settle
9  token.transfer(contract → to)          ← SECOND-TO-LAST
10 bump TTL
```

**The transfer happens after every write has landed.** A hostile or reentrant token sees a fully consistent state in which this spend is *already settled* — so a reentrant `settle` on the same spend is refused by gate 4 rather than paying twice.

**The spend record is re-emitted** at step 8 with `settled` now true. That is what lets a consumer rebuilding from events alone show a paid invoice as paid; one that saw only the authorization event would report two settled payments as still outstanding. `rebuild_test.rs` asserts the rebuilt record matches the stored one, `settled` flag included.

## What "reconciled" means

Not the same as "settled", and the distinction is the whole job of module M16.

| | Set by | Means |
|---|---|---|
| **settled** | the contract, in `settle` | The contract wrote a settlement record and issued a transfer. |
| **reconciled** | `backend/indexer verify` | Somebody **fetched that transaction from the chain** and confirmed it succeeded. |

The contract cannot mark anything reconciled — it has no way to look at its own transaction after the fact. And the indexer never sets it while merely folding events, because seeing an event announcing a settlement is not corroboration of that settlement; it is the claim *being* corroborated.

So `index` builds state, `verify` confirms it against `getTransaction`, and the report says which it is looking at:

```
2 of 2 settlements confirmed against the chain.
```

A `FAILED` transaction is an **error** that stops the run: a settlement record the ledger does not corroborate is the most serious thing this tool can find, and a table with one row quietly marked "not confirmed" is a document that looks like evidence and is not.

A `NOT_FOUND` transaction is reported but not fatal, and the difference matters: a transaction outside the node's retention window is not a fraudulent one, and calling it failed would impugn a perfectly good settlement because the tool was run a month late.

## The lifecycle, end to end

```
authorize_spend                     settle                    indexer verify
      │                               │                             │
      ▼                               ▼                             ▼
  SpendRecord                    Settlement                    Reconciled
  Approved                       spend_id ──┐                  the tx was
  settled=false                  card debited│                 fetched and
      │                          USDC out    │                 it succeeded
      │                          settled=true│
      └──────────────────────────────────────┘
                   one spend, at most one settlement
```

Or, off the happy path:

```
authorize_spend  →  Declined · LimitExceeded  →  a transaction with a hash,
                                                 a record a reviewer can open,
                                                 and no settlement, ever
```
