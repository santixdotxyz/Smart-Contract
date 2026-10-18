# 08 · Events and errors

**These tables are frozen.** `scripts/demo.sh` greps for error numbers exactly and fails the run on a mismatch; `backend/indexer` decodes events by field name; the gateway branches on reason codes. Renumbering a variant or renaming a field breaks all three, silently in at least one of them.

Add at the end. Never renumber.

---

## Two families of refusal

Restated here because everything below depends on the distinction.

| | `Error` | `ReasonCode` |
|---|---|---|
| The call | **reverts** | **succeeds** |
| On chain | no transaction | a real transaction with a hash |
| Where it appears | the entry point's return value | a field on `SpendRecord` |
| A reviewer can open it | no | **yes** |

There is no `Error::CardFrozen`. A frozen card produces a successful transaction carrying `ReasonCode::CardFrozen`, because a reverted transaction leaves nothing for the evidence table to point at.

---

## Error table

Numbers are what the host reports as `Error(Contract, #N)`.

| # | Variant | Raised by | Means |
|---|---|---|---|
| 1 | `AlreadyInitialized` | `initialize` | The contract already has a `Config`. In practice this is every call — see [02](02-contract-interface.md). |
| 2 | `NotInitialized` | entry points needing `Config` | **Unreachable.** `__constructor` runs on every deployment, so there is no route to a deployed-but-unconfigured contract. Kept because the alternative is an `unwrap`, and a defensive `Result` that never fires is cheaper than a panic that fires once. |
| 3 | `Unauthorized` | every privileged entry point | The caller does not hold the required role, or did not sign. |
| 4 | `InvalidAmount` | `deposit` `allocate` `deallocate` `transfer_between_cards` `authorize_spend` `settle` | A non-positive amount. Zero is refused as well as negative. |
| 5 | `CardNotFound` | anything taking a `card_id` | |
| 6 | `OverAllocation` | `allocate` | More than `unallocated`. The deck's `OVER_ALLOCATION`. |
| 7 | `InsufficientCardBalance` | `deallocate` `transfer_between_cards` `settle` | A card was asked for more than it holds. |
| 8 | `NotWhitelisted` | `settle` | The destination is not on the whitelist. |
| 9 | `SpendNotFound` | `settle` `get_spend` | No spend has that id. |
| 10 | `SpendNotApproved` | `settle` | The spend was declined. Only an approved spend can move money. |
| 11 | `AmountExceedsSpend` | `settle` | More than the spend it claims to settle. |
| 12 | `AlreadySettled` | `settle` | Invariant I3. |
| 13 | `LastAdmin` | `revoke_role` | Would leave the contract with no admin — unrecoverable. |
| 14 | `InvalidLimit` | `create_card` `set_limit` | A negative limit. Zero is legal. |
| 15 | `InvalidWindow` | `create_card` `set_limit` | A window of zero seconds, which would reset the limit on every call — or one past `MAX_WINDOW_SECS` (ten years), which would overflow the window's end and leave the card unreadable. See [04](04-cards-and-limits.md). |
| 16 | `SameCard` | `transfer_between_cards` | The same card twice. |
| 17 | `RoleNotFound` | `revoke_role` | The address holds no role. |
| 18 | `InvalidLabel` | `create_card` | Empty, or longer than 32 bytes. |
| 19 | `InvalidMerchantRef` | `authorize_spend` | Empty, or longer than 64 bytes. A spend nobody can attribute is not evidence of anything. |
| 20 | `WhitelistUnchanged` | `add_whitelist` `remove_whitelist` | A no-op. Refused rather than ignored, so a script cannot report success for a change that did not happen. |
| 21 | `SettlementNotFound` | `get_settlement` | No settlement has that id. |
| 22 | `InvalidDestination` | `add_whitelist` `settle` | The destination is the treasury contract itself. |

### 21 and 22 were appended after the table was frozen

Both came out of the adversarial pass, and both are the reason the rule is *append, never renumber*.

**21** replaced a wrong answer rather than adding a new one. `get_settlement` used to report `SpendNotFound` (9) for a missing settlement — a claim about a different ledger, which would send somebody looking for a spend that exists perfectly well.

**22** closes a way to strand money. Settling to the contract's own address is a transfer from the contract to the contract — a no-op — but every write around it still runs: the card is debited and `total_settled` rises, so the amount leaves the card and **never returns to `unallocated`**. The tokens stay in the contract, accounted to nobody, beyond the reach of every entry point. It is refused at `add_whitelist`, where the mistake is made, and again at `settle`, so a whitelist written before the guard existed cannot spend through it.

### Codes the demo asserts

`scripts/demo.sh` drives each of these deliberately and greps for the exact number. A step that failed for a *different* reason stops the run and produces no evidence file.

| Code | Demo step |
|---|---|
| 3 | allocate as somebody who holds no role · settle as the admin rather than the operator |
| 6 | allocate past the unallocated balance |
| 8 | settle to an address that is not whitelisted |
| 11 | settle for more than the spend recorded |
| 12 | settle the same spend twice |

---

## Reason codes

Data, not errors. Carried on `SpendRecord.reason_code`.

**On chain these read as names, not numbers.** The enum has no integer discriminants, so the SDK encodes each variant as its own symbol — `LimitExceeded`, not `2`. That is a requirement, not a style choice: SOW §6 asks a reviewer with minimal technical expertise to open a declined transaction and read why it was refused, and `reason_code: 2` does not clear that bar. See [13-deck-conformance.md](13-deck-conformance.md) §B-5.

| Variant | Check | Deck's name | Means |
|---|---|---|---|
| `None` | — | — | Approved. Carried so every record has a defined reason field. |
| `CardFrozen` | 1 | `CARD_FROZEN` | The card is frozen. Nothing else was consulted. |
| `LimitExceeded` | 2 | `LIMIT_EXCEEDED` | Over what is left of the limit in the current window. Also what a zero-limit card returns. |
| `InsufficientCardBalance` | 3 | `INSUFFICIENT_CARD_BALANCE` | The card's own balance is short. |
| `TreasuryInsolvent` | 4 | `TREASURY_INSOLVENT` | The contract does not hold enough of the token. Reachable only after an issuer clawback — see [07](07-invariants-and-security.md) §I2. |

**The order of these variants is the order of the checks**, and it is frozen. When two conditions fail at once the first one wins, and the off-chain gateway must reproduce the same order or the two systems give different reasons for the same declined transaction.

---

## Event catalogue

Every event is a `#[contractevent]` struct, so its shape lands in the contract's spec and the indexer reads that rather than carrying a hand-written copy of the field order.

### The rule that makes rebuilds possible

**A mutation emits the whole resulting struct, not a delta.**

Deltas are smaller and they are a trap. A `Card` reassembled from a starting balance and a run of `+100 / −40 / +25` is correct only if the consumer saw every one of them, in order, exactly once. Miss a page of RPC results and the number is wrong with nothing to say so.

A stream of complete structs is **idempotent and order-tolerant**: the last event for a card id *is* the card, whatever happened before it and however many times it is replayed. That is what makes decision #08 safe, and it is asserted twice — `rebuild_test.rs` in process, and `state.rs`'s fold against a real network.

### The events

Topics are what `getEvents` can filter on. An id in the data section instead means a consumer wanting one card's history has to fetch and discard everything else to find it.

| Name | Topics | Data | Emitted by |
|---|---|---|---|
| `init` | `founder` | `token` · `decimals` | construction |
| `role` | `who` | `role: Option<Role>` — `None` on revoke | `grant_role` · `revoke_role` |
| `deposit` | `from` | `amount` · `treasury` | `deposit` |
| `card` | `card_id` · `action` | `card` · `treasury` | every card mutation |
| `spend` | `card_id` · `spend_id` | `spend` | `authorize_spend`, **and `settle`** |
| `whitelist` | `who` | `allowed: bool` | `add_whitelist` · `remove_whitelist` |
| `settle` | `spend_id` · `settlement_id` | `settlement` · `treasury` | `settle` |

### `card.action`

A symbol naming which mutation it was, for a human reading the stream and for a consumer that wants to render a *history* rather than a *state*. The card as it now stands is in `card` either way, so a consumer that ignores `action` entirely still reconstructs correctly.

| Action | From |
|---|---|
| `create` | `create_card` |
| `alloc` | `allocate` |
| `dealloc` | `deallocate` |
| `xfer_out` · `xfer_in` | `transfer_between_cards`, one each |
| `limit` | `set_limit` |
| `status` | `set_card_status` |
| `spend` | `authorize_spend`, when the card changed — an approval, or a rolled window |
| `settle` | `settle` |

### Why `spend` is emitted twice

`settle` re-emits the `SpendRecord` with `settled` now true.

Without it, a consumer rebuilding from events would show a paid invoice as still outstanding — it would have seen the authorization and nothing since. `rebuild_test.rs` asserts the rebuilt record matches the stored one, `settled` flag included, and `state.rs`'s fold takes the later event because insert-by-id in ledger order does the right thing for free.

### One subtlety for consumers

Topic fields are **not** repeated in the data map. `whitelist` carries `who` as a topic, so a decoder looking for `who` in the body finds nothing — it reads `topics[1]`. This is exactly what marking a field as a topic is for, and it is the one shape that catches a decoder out. `rebuild_test.rs` covers it deliberately.

---

## What a consumer needs to handle

For a full rebuild from events alone:

1. **Filter by contract.** An indexer pointed at a real network sees every contract in the ledger.
2. **`init` first**, for `decimals` — what every amount is rendered with.
3. **Fold by id.** Last event per `card_id` / `spend_id` / `settlement_id` wins.
4. **`role` with `None` removes** rather than sets. A revoke is "this address holds nothing".
5. **`whitelist` with `allowed: false` removes.** An address added and then removed must rebuild as removed — the case a consumer handling only additions gets wrong, and the one that decides whether a settlement destination looks authorised.
6. **Unknown events are reported, not dropped.** An unrecognised event usually means the consumer is older than the contract it is reading, and silently ignoring one is how a report ends up missing a settlement. `State.unknown_events` collects them and the report prints them.
7. **Then check the books.** The rebuilt cards must sum to `total_allocated − total_settled`. A page lost in transit shows up there and nowhere else — the totals stay perfectly self-consistent either way.
