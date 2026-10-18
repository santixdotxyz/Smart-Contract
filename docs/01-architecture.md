# 01 · Architecture

## Modules

Thirteen on-chain modules (M1–M13), mapped onto seven source files. The mapping is not one-to-one because a module in the deck's sense is a *responsibility*, and two responsibilities that share state belong in one file.

```
smart-contract/src/
  lib.rs         every entry point, and the crate-level explanation of why
  types.rs       the state layout, frozen before the first entry point was written
  errors.rs      the frozen error table                                    M-all
  storage.rs     keys, typed accessors, the TTL discipline                 M-all
  roles.rs       who may do what, and the last-admin guard                 M2
  cards.rs       the limit window and what "remaining" means               M7
  spend.rs       the four checks, in the order that is frozen              M9
  settle.rs      the guards on the only path money leaves by               M10 M11 M12
  events.rs      the event catalogue                                       M13
  invariant.rs   what must hold after every write, and where it is checked M4
```

**Entry points all live in `lib.rs`; the rules they apply live in the module files.** The split is deliberate. `spend::evaluate` and `settle::check` are pure functions — they take state and return a verdict, touching neither storage nor the ledger — which is what lets the test suite enumerate their boundary cases directly, and what would let an off-chain gateway link against the same logic rather than reimplementing it from a specification and getting it subtly different.

The off-chain reader is a separate tree:

```
backend/indexer/src/
  main.rs        three commands: index, verify, report                     M16
  rpc.rs         getEvents, getTransaction, getHealth, getLatestLedger
  scval.rs       reading Soroban values back out of XDR
  model.rs       the contract's types, as the indexer sees them
  state.rs       the fold: an event stream in, the contract's state out
  report.rs      the reconciliation table, rendered for a reader
```

### Why two Cargo workspaces

`smart-contract/` targets `wasm32v1-none` with an aggressive `[profile.release]`. One line of it is load-bearing:

```toml
overflow-checks = true
```

Rust turns overflow checks **off** in release by default. Without that line `balance - amount` would wrap silently in the deployed wasm while every test passed in debug — decision #05 promises checked arithmetic, and this is where the promise is actually kept.

`[profile.release]` only takes effect at a **workspace root**. Making `backend/` a member of the contract's workspace would drop the whole block: the wasm would grow, the overflow checks would go off, and nothing would fail to say so.

## Storage layout

| Key | Type | Durability | Why |
|---|---|---|---|
| `Config` | `Config` | instance | Read on any call that touches the token. Tiny, and rides with the instance. |
| `Treasury` | `Treasury` | instance | Three integers, read and written on nearly every call. |
| `Roles` | `Map<Address, Role>` | instance | Read on every write. Small — two or three entries in practice. |
| `Whitelist` | `Map<Address, ()>` | instance | A set; the value carries nothing, membership is the whole fact. |
| `CardSeq` `SpendSeq` `SettlementSeq` | `u32` | instance | Monotonic id counters. |
| `Card(id)` | `Card` | persistent | Grows without bound with the number of categories. |
| `Spend(id)` | `SpendRecord` | persistent | Grows without bound with activity. |
| `Settlement(id)` | `Settlement` | persistent | Likewise. |
| `SpendSettlement(spend_id)` | `u32` | persistent | The index behind invariant I3. |

**Instance for what the contract needs to act; persistent for what grows.** An instance entry is loaded on every invocation whether or not it is used, so putting a thousand cards there would make every call pay for all of them.

### Every read goes through a named function

`storage.rs` exposes `card(e, id)`, `set_card(e, &card)`, and so on. No entry point writes `e.storage().persistent().get(&DataKey::Card(id))` directly.

The reason is durability. Instance and persistent entries archive on different schedules, and an archived card is a card whose money cannot be moved — a failure that needs no bug and no attacker, just eight quiet months. Routing every access through one place means the durability of a thing is decided once.

### Enumeration is a range walk

Ids are dense from 1 to the sequence value, so `list_cards` walks `1..=card_count` and skips anything missing. There is no stored index to keep consistent, and an archived entry looks the same as a missing one — the walk skips both rather than failing, so one archived card does not make `list_cards` unusable for the rest.

Ids start at **1**, which is what lets `0` mean "all cards" in `list_spends` without an `Option` in the public interface. The CLI cannot express an `Option<u32>` conveniently, and the demo script has to.

## TTL and archival — decision #08

The deck offers a choice: bump TTL on every write, or let historical records live in events and be reconstructed off-chain. The answer is **both**, because they solve different problems.

**Bumping** keeps alive the entries an entry point needs in order to *act*: config, roles, treasury, whitelist, cards. Losing one of those breaks the contract. Every write extends the keys it touched:

```
threshold   30 days   extend when fewer than this many ledgers remain
amount      90 days   extend back out to this
```

Both well inside the network's 180-day maximum, so the extension is never silently clamped to something shorter than the code claims.

**Events** are what the long history rests on. Spend and settlement records are bumped too, but the guarantee that history survives comes from `backend/indexer`, which rebuilds every record from the event stream alone. That property is asserted twice: in-process by `rebuild_test.rs`, and against a real network by `state.rs`'s fold — and it is module M13's definition of done.

**The bump is bounded.** A call extends the keys it actually touched, never every card in the contract. An unbounded bump grows with the number of cards until an entry point no longer fits in a transaction.

## The write-order discipline

Every write function runs the same ten steps, in the same order.

```
 1 require_auth on the correct role
 2 validate parameters
 3 roll the limit window if it has expired
 4 load state
 5 guards, cheap → expensive        storage reads before cross-contract calls
 6 apply changes
 7 assert invariants
 8 emit events
 9 token transfer                   ALWAYS SECOND-TO-LAST
10 bump TTL                         ALWAYS LAST, and BOUNDED
```

Three steps are load-bearing.

**Step 3, before anything is evaluated.** A card whose window lapsed while nobody was calling must be judged against the period it is actually in, not the one it was last written in. Rolling after the checks would decline spending that a fresh period allows.

**Step 5, cheap before expensive.** `authorize_spend` reads the token contract's balance, which is a cross-contract call. The three checks that need no network run first, so a spend refused on a frozen card costs a storage read rather than an invocation.

**Step 9, second-to-last.** This is the reentrancy mitigation. A hostile or reentrant token sees a fully consistent, fully committed state — in `settle`, one in which this spend is already marked settled, so a reentrant call is refused with `AlreadySettled` rather than paying twice.

**Step 10, last and bounded.** See above.

## Two families of refusal

The single most important distinction in the contract, and the easiest to flatten.

| | `Error` | `ReasonCode` |
|---|---|---|
| What happens | The call reverts. Nothing is written. | The call **succeeds**. A record is written saying what was refused. |
| Where it appears | The return value of the entry point | A field on `SpendRecord` |
| On chain | No transaction — the CLI simulates before submitting, so it never reaches the ledger | A real transaction with a real hash |
| Example | A non-admin calls `allocate` → `Unauthorized` | A frozen card → `decision: Declined, reason_code: CardFrozen` |

There is no `Error::CardFrozen`. A frozen card produces a successful transaction, because a reverted one leaves nothing for the evidence table to point at — and SOW §6 asks a reviewer to open a declined spend and read the reason.

`authorize_spend` reverts for exactly three things, all of which mean there is nothing sensible to record: the caller is not an operator, the card does not exist, the amount or merchant reference is not usable. Every *business* refusal — frozen, over limit, short balance, treasury unable to pay — returns `Ok` with a declined record.

## Why every entry point returns `Result`

`try_*` on the generated client yields `Err(Ok(Error::X))` only for functions declared to return `Result`. With `panic_with_error!` a test can assert nothing more precise than "it failed".

That precision is not academic here. `scripts/demo.sh` greps for `Error(Contract, #N)` by exact number and **fails the run on a mismatch**; the indexer branches on the reason code; the gateway branches on both. A test that cannot tell code 6 from code 8 is not testing the thing that matters.

## Governance: there is none

No upgrade path. No admin override. No pause switch. `Config` is written once at construction and never again — the token included.

The highest privilege that exists is `Admin`, and the one guard that protects the contract from itself is that **the last admin cannot be revoked**. A treasury with no admin can never create a card, allocate, or manage the whitelist again. The money is not stolen; it is stranded, permanently, with no recovery path.

Two roles and no implicit promotion: an admin is not a lesser operator and an operator is not a lesser admin. They guard different things — the shape of the money versus its movement — and an implicit promotion would make the negative tests for each role pass for the wrong reason.

The one shared entry point is `set_card_status`. An admin freezes a card as policy; an operator freezes one because something is happening right now, and making them wait for an admin is the wrong trade in the single case where speed is the point.
