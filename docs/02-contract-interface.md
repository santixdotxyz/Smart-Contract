# 02 · Contract interface

Every entry point: who may call it, what it checks and in what order, what it changes, what it emits, and how it can be refused.

**This is the contract between the contract, the gateway, the indexer and the demo script.** Changing a signature here changes all four.

## A note on `caller`

The deck's API surface (slide 10) writes entry points without a caller parameter — `allocate(card_id, amount)`. Soroban needs the address to call `require_auth` on, so every privileged entry point takes `caller: Address` first. This is a mechanical difference, not a design one: the address named is the one that must sign.

`deposit(from, amount)` matches the deck exactly, and for a reason worth keeping in mind — the contract does not *pull* money, it asks the token to move money the depositor has already authorised.

## Reading the tables

- **Auth** — the role `require_auth` is checked against. `—` means anyone.
- **Refuses with** — errors that revert the call. See [08-events-and-errors.md](08-events-and-errors.md) for the numbers.
- Gates are listed **in execution order**. Order decides which refusal a caller sees when several apply.

---

## M1 · Configuration

### `__constructor(admin, token_addr, operators) `

Deployment and construction in **one transaction**. `stellar contract deploy -- <constructor args>` is the form `deploy.sh` uses, and the reason is a window rather than a convenience: deploying first and initialising second leaves an interval — a few seconds, entirely sufficient — in which anybody watching the ledger can call `initialize` and name themselves admin of a contract somebody else paid to deploy.

| | |
|---|---|
| Auth | none — whoever deploys |
| Reads | `token_addr.decimals()`, **never** the deployer's claim about it |
| Writes | `Config { founder, token, decimals }`, an empty `Treasury`, `admin` as `Admin`, each operator as `Operator` |
| Emits | `init` |

`decimals` is read from the token contract because a wrong value in `Config` would make every amount an explorer renders wrong by orders of magnitude, with nothing on chain to contradict it.

### `initialize(admin, token_addr, operators) -> Result<()>`

The deck's `initialize`, kept as a real entry point — **and it always refuses**.

Because `__constructor` exists the host runs it on every deployment and refuses a deployment that omits its arguments, so by the time any transaction can reach `initialize` the contract is already configured. That is the point rather than an accident: the deck's API surface lists `initialize` and M1 asks us to demonstrate a second call is refused; the constructor is what removes the window in which somebody else could have made the *first* call.

| | |
|---|---|
| Refuses with | `AlreadyInitialized` (1) — always, in practice |

---

## M2 · Roles

### `grant_role(caller, who, role) -> Result<()>`

| | |
|---|---|
| Auth | `Admin` |
| Writes | `Roles[who] = role` |
| Emits | `role` carrying `Some(role)` |
| Refuses with | `Unauthorized` (3) |

### `revoke_role(caller, who) -> Result<()>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · `who` holds a role → else `RoleNotFound` (17)<br>2 · not the **last** admin → else `LastAdmin` (13) |
| Writes | removes `Roles[who]` |
| Emits | `role` carrying `None` |

The last-admin guard is the only thing standing between an ordinary mistake and a permanently unusable treasury. There is no upgrade path and no recovery address.

Revoking the last **operator** is allowed: a treasury with no operator cannot authorise or settle, but an admin can appoint a new one. It is recoverable, so it is not guarded.

---

## M3 · Deposit

### `deposit(from, amount) -> Result<Treasury>`

| | |
|---|---|
| Auth | `from` — anyone may fund the treasury |
| Gates | 1 · `amount > 0` → else `InvalidAmount` (4)<br>2 · configured → else `NotInitialized` (2) |
| Writes | `total_deposited += amount` |
| Emits | `deposit` carrying the depositor, the amount and the resulting treasury |
| Transfers | `amount` of the configured token, `from` → contract |

There is **no token parameter**. "Only accepts the configured asset" is true by construction rather than by check — the token was fixed at construction and there is nothing to pass wrong.

Zero is refused as well as negative: a zero-value deposit is a transaction that says nothing and a row the reconciliation table has to explain.

---

## M5 · Card registry

### `create_card(caller, category, label, limit, window_secs) -> Result<Card>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · label present and ≤ 32 bytes → else `InvalidLabel` (18)<br>2 · `limit >= 0` → else `InvalidLimit` (14)<br>3 · `0 < window <= MAX_WINDOW_SECS` after defaulting → else `InvalidWindow` (15) |
| Writes | a new `Card` at the next id, balance 0, status `Active`, window opening now |
| Emits | `card` with action `create` |

`category` is a **`Category` enum**, not a string. The deck is emphatic (slide 12) and the reason is downstream: a report that groups spending by category can only be clean if the category is drawn from a closed set. `Other` exists so a real expense never has to be mislabelled into one of the four named ones.

`window_secs = 0` means "use the 30-day default" rather than being an error, so a caller with no opinion about decision #02 does not have to have one.

A `limit` of `0` is legal and means the card may not spend. It is the resting state of a card that exists but is not yet budgeted — and meaningfully different from frozen: a zero-limit card declines with `LimitExceeded`, which says the card is fine and the budget is not.

---

## M6 · Allocation

### `allocate(caller, card_id, amount) -> Result<Card>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · `amount > 0` → `InvalidAmount` (4)<br>2 · card exists → `CardNotFound` (5)<br>3 · `amount <= unallocated` → **`OverAllocation` (6)** |
| Writes | `total_allocated += amount`, `card.balance += amount` |
| Emits | `card` with action `alloc` |

Gate 3 is the guard everything downstream assumes held. Every balance check in `authorize_spend` and every debit in `settle` is only meaningful because a card can never hold money the treasury does not have.

The check is `>`, not `>=` — allocating exactly the whole unallocated pool is legal. An off-by-one here would strand the last unit of every treasury.

### `deallocate(caller, card_id, amount) -> Result<Card>`

The reverse. Gate 3 becomes `amount <= card.balance` → `InsufficientCardBalance` (7). Emits `card` with action `dealloc`.

A **frozen** card can still be deallocated. Freezing stops spending; a frozen card whose funds were also stuck would make freezing a decision nobody wants to take.

### `transfer_between_cards(caller, from_id, to_id, amount) -> Result<()>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · `amount > 0` → `InvalidAmount` (4)<br>2 · `from_id != to_id` → `SameCard` (16)<br>3 · both cards exist → `CardNotFound` (5)<br>4 · `amount <= from.balance` → `InsufficientCardBalance` (7) |
| Writes | moves `amount` between the two card balances |
| Emits | `card` twice, with actions `xfer_out` and `xfer_in` |

**The treasury totals do not move.** This is a transfer inside the allocated pool, so the sum of card balances is unchanged and no solvency question arises.

---

## M7 · Limits

### `set_limit(caller, card_id, limit, window_secs) -> Result<Card>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · `limit >= 0` → `InvalidLimit` (14)<br>2 · card exists → `CardNotFound` (5)<br>3 · if `window_secs != 0`, `0 < it <= MAX_WINDOW_SECS` → `InvalidWindow` (15). **Validated before anything is mutated.** |
| Writes | rolls the window if expired, then sets `limit`, and `limit_window_secs` if given |
| Emits | `card` with action `limit` |

Changing the period does **not** reset `spent_in_window`. Shortening a window that has already been spent against would otherwise hand the whole limit straight back, turning a tightening into a loosening.

`window_secs = 0` means "leave the period alone", not "zero seconds" — which would make the limit meaningless by resetting on every call.

---

## M8 · Status

### `set_card_status(caller, card_id, status) -> Result<Card>`

| | |
|---|---|
| Auth | `Admin` **or** `Operator` — the only entry point either role may call |
| Gates | card exists → `CardNotFound` (5) |
| Writes | `card.status` |
| Emits | `card` with action `status` |

---

## M9 · Spend authorization

### `authorize_spend(caller, card_id, amount, merchant_ref) -> Result<SpendRecord>`

The centre of the contract. **A decline is a success.**

| | |
|---|---|
| Auth | `Operator` |
| Refuses with | `Unauthorized` (3) · `CardNotFound` (5) · `InvalidAmount` (4) · `InvalidMerchantRef` (19) — and nothing else |
| Reads | the card, the treasury, and the token's **real** balance |
| Writes | a `SpendRecord`; on approval, `card.spent_in_window += amount` |
| Emits | `spend`, plus `card` if the card changed |
| Transfers | **nothing** |

The four checks, in the order that is frozen:

| # | Check | Reason code if it fails |
|---|---|---|
| 1 | `status == Active` | `CardFrozen` |
| 2 | `amount <= limit - spent_in_window` (after rolling the window) | `LimitExceeded` |
| 3 | `amount <= card.balance` | `InsufficientCardBalance` |
| 4 | the contract actually holds enough of the token | `TreasuryInsolvent` |

Why the order is frozen, and why it matters more than the checks: two conditions often fail together — a frozen card is usually also over its limit, because that is why somebody froze it. Which reason comes back is decided **entirely** by the order. The off-chain gateway applies the same four rules from its own cached copy of the state, and if the two disagree about the order they give different reasons for the same declined transaction, and the reconciliation table stops explaining anything.

**No money moves.** `spent_in_window` rises on approval; `balance` does not change until `settle`. That is the race window decision #01 accepts: two approvals can both pass check 3 before either settles. See [05-spend-authorization.md](05-spend-authorization.md).

---

## M10 · Whitelist

### `add_whitelist(caller, who) -> Result<()>` · `remove_whitelist(caller, who) -> Result<()>`

| | |
|---|---|
| Auth | `Admin` |
| Gates | 1 · `who` is not the contract itself → else `InvalidDestination` (22)<br>2 · the change must actually change something → else `WhitelistUnchanged` (20) |
| Emits | `whitelist` carrying `allowed` |

A no-op is refused rather than silently succeeding. A script that reports "whitelisted" for a call that changed nothing is a script whose output cannot be trusted as evidence.

Removing an address does **not** touch settlements already made to it. A reconciliation record is a statement about what happened, and rewriting history to match current policy would make the ledger useless as evidence.

---

## M11 · M12 · Settlement

### `settle(caller, spend_id, to, amount) -> Result<Settlement>`

The only entry point that moves money out, and the one a reviewer opens first.

| | |
|---|---|
| Auth | `Operator` |
| Reads | the spend, the originating card, the whitelist, the settlement index |
| Writes | `card.balance -= amount`, `total_settled += amount`, `spend.settled = true`, a new `Settlement` |
| Emits | `card` (action `settle`), `spend` re-emitted with `settled` true, and `settle` |
| Transfers | `amount` of the configured token, contract → `to` |

The gates, in order:

| # | Check | Refuses with |
|---|---|---|
| 0 | caller is an operator | `Unauthorized` (3) |
| 0 | `to` is not the contract itself | `InvalidDestination` (22) |
| 0 | the spend exists | `SpendNotFound` (9) |
| 1 | `amount > 0` | `InvalidAmount` (4) |
| 2 | `to` is whitelisted | `NotWhitelisted` (8) |
| 3 | `spend.decision == Approved` | `SpendNotApproved` (10) |
| 4 | not settled already | `AlreadySettled` (12) |
| 5 | `amount <= spend.amount` | `AmountExceedsSpend` (11) |
| 6 | `amount <= card.balance` | `InsufficientCardBalance` (7) |

The deck names three guards. Five more are required for those three to mean anything — a declined spend that could be settled would make the refusal decorative, and gate 6 is what makes the contract the final arbiter of the race window.

**The debit comes off the originating card**, not the unallocated pool. The SOW is explicit, and it is the requirement most likely to be got wrong under time pressure because taking it from unallocated is simpler and the top-line total comes out the same. It is not the same: the card balance is the record of what a category has left, and settling elsewhere leaves every category report wrong while the headline number looks right.

**Settling for *less* than the spend is allowed.** A real settlement often is less — a partial capture, a discount applied after authorization — and refusing it would force the operator to choose between over-paying and leaving the spend permanently open. The spend closes at whatever figure it closed for; there is no partial state to chase.

**`total_allocated` does not move on settle.** That is what keeps `Σ card.balance = total_allocated − total_settled` true. See [03-treasury-accounting.md](03-treasury-accounting.md).

---

## Views

None of these write, and none charges a transaction.

| Entry point | Returns |
|---|---|
| `get_config()` | `Config` — founder, token, decimals |
| `get_treasury()` | the three running totals |
| `get_unallocated()` | `total_deposited − total_allocated` |
| `get_allocated_outstanding()` | `total_allocated − total_settled` — what the card balances must sum to |
| `list_cards()` | every card |
| `get_card(card_id)` | one card, **as stored** |
| `get_card_live(card_id)` | one card with an expired window already rolled |
| `get_remaining_limit(card_id)` | headroom now, with the window rolled |
| `list_spends(card_id)` | one card's history, or all of them when `card_id == 0` |
| `get_spend(spend_id)` | one record |
| `list_settlements()` | the reconciliation ledger |
| `get_settlement(id)` | one settlement, or `SettlementNotFound` (21) |
| `get_settlement_of_spend(spend_id)` | the settlement that closed a spend, if any — the reverse lookup a reviewer follows |
| `is_whitelisted(who)` · `list_whitelist()` | the destination whitelist |
| `role_of(who)` | `Some(Role)` or `None` |

### `get_card` versus `get_card_live`

`get_card` returns what storage holds. `get_card_live` rolls an expired window first.

Both exist because they answer different questions. A tool auditing what the chain *says* wants the stored value; a console about to warn a user that they are out of limit wants the live one, or it will show last period's figure for a window that has already reset. Neither writes — the roll is persisted by the next call that changes anything.

## What is deliberately absent

No `pause`. No `upgrade`. No `set_token`. No `withdraw_all`. No admin escape hatch of any kind.

Every one of those would be useful in an emergency and every one is a key that can be stolen. Phase 1 has no security audit, so the answer to "what if the admin key is compromised" is bounded by what an admin can do — and an admin can move money between categories and out to **whitelisted addresses only**. See [07-invariants-and-security.md](07-invariants-and-security.md).
