# 07 · Invariants and security

## Three invariants, checked in three different places

The deck states two. There are three, and — more importantly — they are not all the *kind* of property that can be asserted.

### I1 · The accounting identity

```
unallocated    = total_deposited − total_allocated  ≥ 0
Σ card.balance = total_allocated − total_settled    ≥ 0
```

**Checked inside the contract, after every write**, by `invariant::assert_books`. It is three integer comparisons, which is what makes it affordable on every call — see [03-treasury-accounting.md](03-treasury-accounting.md) for why the running totals exist rather than summing the cards.

It **panics** rather than returning an error, and that is deliberate. An `Error` is something a caller did; this is something the *contract* did, and no caller behaviour should produce it. A numbered error code here would invite the gateway to branch on a bug.

What it cannot catch: the totals are internally consistent by construction. If `allocate` raised `total_allocated` and forgot to credit the card, they would still agree and the assertion would pass. Only summing the actual cards catches that — which the test harness does after every scenario.

### I2 · Backing

```
total_deposited − total_settled ≤ token.balance(contract)
```

*The contract holds at least what it owes.*

**Not asserted after writes**, and the difference from I1 is the point:

- I1 is about our own arithmetic. It can only break if the code is wrong.
- I2 is about the outside world. On Stellar an issuer with clawback enabled can remove tokens from any holder, this contract included, and **no bug on our side is involved**.

Panicking on I2 would freeze a treasury that is merely short: nobody could deallocate, nobody could freeze a card, nobody could wind the thing down. So I2 is *consulted* rather than asserted — it is check 4 of `authorize_spend`, and failing it produces `ReasonCode::TreasuryInsolvent` on a recorded, declined spend.

The contract keeps working when it is short. It refuses to spend and says why, and every operation that does not need the missing money still runs. `deck_conformance.rs::b3_a_short_treasury_declines_rather_than_freezing` asserts exactly that.

The deck writes its invariant as an equality between the books and the token balance. Treated as an assertion that would be wrong for the reason above; treated as a comparison it is right. This is registered as a deviation in [13-deck-conformance.md](13-deck-conformance.md) §B-3.

### I3 · No double settlement

Each `spend_id` appears at most once in the settlement ledger.

**Enforced structurally**, by the `SpendSettlement(spend_id)` key: `settle` refuses with `AlreadySettled` if it exists. There is no scan and no window in which two settlements for one spend could both pass their checks.

## What the contract cannot check, and the tests must

Two things, and both are where a serious bug would hide.

**The sum of the actual cards.** Covered above.

**The token's real balance.** No assertion inside the contract can see it. A `settle` that updated every record and never issued the transfer looks perfect from the inside — the books balance, the invariants hold, and the money is somewhere else.

Both live in `harness::F::check_books`, which every scenario in the suite ends with:

```rust
Σ card.balance (summed from the actual cards) == total_allocated − total_settled
Σ card.balance + unallocated                  == total_deposited − total_settled
token.balance(contract)                       == total_deposited − total_settled − clawed_back
every settled spend has exactly one settlement, and no spend has two
```

See [10-testing.md](10-testing.md).

## Threat model

### There is no governance to attack

`Config` is written once at construction and never again — the token included. No upgrade path. No admin override. No pause switch. No `withdraw_all`.

Every one of those would be useful in an emergency, and every one is a key that can be stolen. Phase 1 has no security audit, so the honest answer to *"what if a key is compromised"* is to bound what any key can do rather than to add a stronger key.

### What a compromised **operator** key can do

The operator signs `authorize_spend` and `settle`. It is a hot key by necessity — it has to answer in seconds — and decision #06 accepts that, recorded here as technical debt for the deferred security review.

An attacker holding it can:

- authorise spends on any active card, up to its limit and balance;
- settle approved spends to a **whitelisted address only**;
- freeze cards.

They cannot: change the whitelist, create cards, allocate, change limits, or move money anywhere an admin has not already approved.

**The whitelist is the bound**, and it is the most valuable guard in the contract for exactly this reason. Stolen operator keys drain to attacker addresses; that is the shape of the attack, and it is the shape the whitelist blocks. The residual exposure is the balance of cards whose spends can be authorised and settled to an address the organisation itself controls — recoverable, and visible in the reconciliation ledger the moment anybody looks.

### What a compromised **admin** key can do

An admin can create cards, allocate the entire unallocated pool onto one, set any limit, and **add an address to the whitelist**. Combined with the operator role — or by granting itself one — that is the whole treasury.

This is not mitigated in Phase 1 and should not be presented as if it were. The mitigations that belong here are operational: a multisig account as the admin, and a separate key for the operator so the two are not compromised together. `deploy.sh` takes both addresses separately for that reason. It is listed in [12-roadmap.md](12-roadmap.md) as work for Phase 2.

### The last admin cannot be revoked

The one guard protecting the contract from an ordinary mistake rather than from an attacker.

A treasury with no admin can never create a card, allocate, or manage the whitelist again. There is no upgrade path and no recovery address — the money is not stolen, it is **stranded, permanently**. `revoke_role` refuses with `LastAdmin` rather than allowing it.

Revoking the last *operator* is allowed, because it is recoverable: an admin can appoint a new one.

### Reentrancy

Every token transfer is **step 9 of 10** — second to last, after every state write has landed and before only the TTL bump.

A hostile or reentrant token therefore sees a fully consistent, fully committed state. In `settle` specifically, it sees one in which this spend is already marked settled, so a reentrant `settle` on the same spend is refused by the `AlreadySettled` gate rather than paying twice.

### Arithmetic

`i128` in the token's smallest unit throughout. Every subtraction is guarded by a check that runs first, so the amount that would underflow is refused with a named error rather than wrapping.

`overflow-checks = true` in the release profile is the backstop. Rust turns overflow checks **off** in release by default, so without that line a wrap would happen only in the deployed wasm while every test passed in debug. It is set at the workspace root — which is why `backend/` is a separate workspace, since `[profile.release]` only takes effect at a root and merging them would drop the block with nothing failing.

### Storage archival

Persistent entries archive. An archived card is a card whose money cannot be moved.

Mitigated on both sides (decision #08): TTL is bumped on every touched key, bounded to the keys the call actually named; and the durable history lives in **events**, from which `backend/indexer` rebuilds all state. `rebuild_test.rs` asserts the reconstruction in-process; the demo asserts it against a real network.

### Denial of service by enumeration

`list_cards`, `list_spends` and `list_settlements` walk from 1 to the sequence value. On a treasury with tens of thousands of spend records, `list_spends(0)` will not fit in a transaction.

This is a real limit and it is accepted for Phase 1: these are view functions, they cost nobody anything on chain, and the tool that actually needs the full history — the indexer — reads events rather than calling them. A console that needed paging would be a Phase 2 concern, and it is listed as one.

## Accepted risks, stated plainly

| Risk | Why it is accepted | Where it is written down |
|---|---|---|
| The authorization race window — two approvals can exceed a card | A hold needs an expiry, and an expiry needs a keeper. The contract never over-pays; the gateway owns the orphan. | decision #01, [05](05-spend-authorization.md), `spend_tests.rs` |
| The operator key is hot | It has to answer in seconds. Bounded by the whitelist. | decision #06 |
| A compromised admin key is the whole treasury | No mitigation in Phase 1. Use a multisig. | this document, [12](12-roadmap.md) |
| No security audit | Explicitly out of scope, SOW §4.1 | [00](00-overview.md) |
| Unbounded `list_*` views | Views cost nothing on chain; the indexer reads events | above |
| **No mainnet** | Deferred until after a security review that has not happened | everywhere, deliberately |

## What would break the design

Written down so a future change has to argue against it rather than drift past it.

- **Debiting the card on authorization instead of settlement.** Leaves a card short for a payment that never happened.
- **Decrementing `total_allocated` in `settle`.** Makes `unallocated` grow by every settlement — money appearing from nowhere.
- **Asserting I2 instead of consulting it.** Turns a clawback into a permanently frozen treasury.
- **Reverting on a business decline.** Removes the transaction a reviewer is supposed to open, and with it the §6 evidence.
- **Changing the order of the four checks.** Splits the contract and the gateway on which reason a decline gets.
- **An unbounded TTL bump.** Grows with the card count until an entry point stops fitting in a transaction.
