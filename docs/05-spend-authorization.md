# 05 · Spend authorization

## The split, and why it exists

A card network wants an answer in about two seconds. A Stellar ledger closes in about five. The contract cannot be in the path of a swipe.

```
OFF-CHAIN · milliseconds
  card network → gateway → applies four rules from cached state → answers

ON-CHAIN · ≥ 1 ledger later
  gateway → authorize_spend → the same four rules, against live state
                            → a SpendRecord, approved or declined
```

The contract is where the rules are **defined** and where the decision is **recorded**. The gateway is where it is *delivered in time*. Two systems apply the same rules to the same question, and everything difficult about this design follows from the gap between them.

## A decline is a successful transaction

The most important sentence in this document.

`authorize_spend` returns `Ok` whether it approves or refuses. A refusal writes a `SpendRecord` with `decision: Declined` and a `reason_code` saying why, and that transaction lands on the ledger with a hash anybody can open.

It reverts for exactly four things, all of which mean **there is nothing sensible to record**:

| | Error |
|---|---|
| the caller is not an operator | `Unauthorized` (3) |
| the card does not exist | `CardNotFound` (5) |
| the amount is not a positive number of tokens | `InvalidAmount` (4) |
| the merchant reference is empty or over 64 bytes | `InvalidMerchantRef` (19) |

Every *business* refusal is recorded instead. This is decision #04, and the reasoning is SOW §6: a reviewer with minimal technical expertise must be able to open a declined spend and read why it was refused. A reverted transaction leaves nothing to open. The deck's own Spend Simulator wireframe shows `DECLINED · LIMIT_EXCEEDED` next to a transaction hash — which is only possible if the decline is a transaction.

It costs persistent storage, and the deck floats emitting an event instead. The evidence requirement decides it.

## The four checks

In this order, and the order is frozen.

| # | Check | Reason code |
|---|---|---|
| 1 | `status == Active` | `CardFrozen` |
| 2 | `amount <= limit − spent_in_window` | `LimitExceeded` |
| 3 | `amount <= card.balance` | `InsufficientCardBalance` |
| 4 | the contract actually holds enough of the token | `TreasuryInsolvent` |

The window is rolled **before** check 2 runs, so a card whose period lapsed while nobody was calling is judged against the period it is actually in.

### Why the order matters more than the checks

Two conditions fail together more often than not. A frozen card is usually also over its limit — that is generally *why* somebody froze it. Which reason comes back is decided entirely by which check runs first.

The gateway applies the same four rules from its own cached copy of the state. If the two disagree about the order, they give **different reasons for the same declined transaction**, and the reconciliation table stops explaining anything: the customer was told one thing, the ledger says another, and nobody can tell which is the bug.

So the order is part of the interface, not an implementation detail. It is stated here, in `src/spend.rs`, in the `ReasonCode` enum's variant order, and asserted by `spend_tests.rs::the_check_order_decides_the_reason_when_several_fail` — which sets up a card failing all four at once and walks the reasons back one at a time.

### Cheapest and most categorical first

Check 1 is a policy decision and no arithmetic should override it. Checks 2 and 3 are integer comparisons on state already loaded. Check 4 is a **cross-contract call** to the token, so it runs last: a spend refused on a frozen card costs a storage read rather than an invocation.

### Check 4 reads the token, not the books

This is the one that looks redundant and is not.

The contract's own accounting guarantees the treasury covers every card — that is invariant I1, and it holds after every write. So if check 4 consulted the running totals it could never fire, and `TreasuryInsolvent` would be dead code.

It reads `token.balance(contract_address)` instead, and becomes reachable in exactly one situation: **tokens left the contract without its knowledge.** On Stellar that means an issuer with clawback enabled, which real assets do have. The books cannot see a clawback, and the books are exactly what would be wrong if they could.

So check 4 asks the token directly, and a shortfall surfaces as a recorded decline rather than a panic — see [07-invariants-and-security.md](07-invariants-and-security.md) §I2 for why the contract keeps working when it is short rather than freezing.

## Nothing moves

`authorize_spend` changes exactly one thing on approval:

```
card.spent_in_window += amount
```

`card.balance` is untouched. The money is still on the card and still counted by the treasury. It moves later, in `settle`, when a payment has actually been made off-chain and there is a whitelisted address to send it to.

That is not a simplification — it is what the two-phase design requires. An authorization is a *promise*, and the promise may never be kept: a merchant may not capture, or the gateway may reverse it. Debiting on authorization would leave the card short for a payment that never happened, with no state to say so.

## The race window — decision #01

Because nothing is held, two authorizations against one card can both pass check 3.

```
card balance 60

  authorize 40  → Approved      balance still 60, spent_in_window 40
  authorize 40  → Approved      balance still 60, spent_in_window 80

  settle #1 for 40 → ok         balance 20
  settle #2 for 40 → REFUSED    InsufficientCardBalance
```

The deck offers two ways out: an on-chain hold or reserve, or accepting that the contract is the final arbiter and making the gateway handle the orphan. **We accept.** The reasons:

- A hold needs an expiry, and an expiry needs something to run at the deadline. Nothing on Stellar wakes a contract up, so an expiring hold means a keeper transaction per hold — an operational dependency whose failure mode is money locked on a card for a payment that was abandoned.
- A hold that never expires is worse: one abandoned authorization strands its amount permanently.
- The gateway is where the race actually happens, it has the state to reason about it in milliseconds, and it is the only party that can decline the second swipe *before* it is made rather than after.

What the contract guarantees instead is that **it never over-pays**: `settle` re-checks the card balance, and the second settlement is refused. The orphan is visible — an approved, unsettled `SpendRecord` — and it is the gateway's problem to reconcile.

This is an accepted risk, stated rather than hidden. `spend_tests.rs::two_approvals_can_exceed_the_card_and_settlement_is_the_arbiter` is the test that pins the behaviour down, so if a future change silently introduces a hold, that test fails and somebody has to decide it deliberately.

## What the record carries

```rust
struct SpendRecord {
    id, card_id, amount,
    merchant_ref,          // who was being paid. Bounded at 64 bytes
    decision,              // Approved | Declined  — an enum, not a bool
    reason_code,           // None | CardFrozen | LimitExceeded | …
    settled: bool,         // set by settle. Enforces invariant I3
    ledger_seq: u32,       // the ledger this was written in
    timestamp: u64,        // and its wall clock
}
```

`amount` on a declined record is what was **asked for**, not what moved — nothing moved.

`decision` is an enum rather than a boolean on purpose. A real card network has authorization, capture, void, refund and chargeback; Phase 1 models the first two outcomes only, and the deck asks explicitly (slide 15) that we not close the door on the rest. Adding `Captured` to an enum is a code change; adding it to a `bool` is a storage migration.

`ledger_seq` exists because of the deck's fifth UX principle: *"approved" and "written on chain" are two different moments, at least one ledger apart.* A console that collapses them is lying about the race window, so the contract stamps the moment it actually wrote.

## Reason codes are names, not numbers

On chain the reason reads `LimitExceeded`, not `2`.

Giving the enum integer discriminants would make the SDK encode it as a `u32`, and a reviewer opening a declined transaction would see `reason_code: 2` where the requirement is that they can read *why*. As a plain enum it encodes as its own name — which is the deck's `LIMIT_EXCEEDED`, in the vocabulary the SOW's evidence table uses.

This was found by running the demo: the first version had discriminants, and the acceptance script's assertion `expected 'CardFrozen', got '1'` is what surfaced it. See [13-deck-conformance.md](13-deck-conformance.md) §B-5.

## What the gateway has to reproduce

Everything in this document, exactly:

1. the four checks, **in this order**;
2. the lazy window roll before check 2;
3. `remaining_limit` clamped at zero, so a lowered limit does not read as headroom;
4. the reason code names.

[09-offchain-integration.md](09-offchain-integration.md) is the handover. `src/spend.rs::evaluate` is a pure function taking state and returning a verdict — it is deliberately shaped so a gateway can link against it rather than reimplementing it from prose and getting it subtly different.
