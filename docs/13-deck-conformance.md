# 13 · Deck conformance — the audit record

The only document organised by the **deck** rather than by the contract.

[`resources/specs/Saintix_SOW1_Deck.html`](../resources/specs/Saintix_SOW1_Deck.html) is the source of truth. It is the client's build brief and it is **written in Vietnamese** — the one non-English file in this repository, kept because an audit nobody can check against the original is not an audit. References to *slide N* point into it, numbered in display order. This file goes through it and, for every claim, says whether the shipped contract meets it and **names the test that proves it**. Where we deliberately built something else, §B says what and why.

If you are checking whether the contract meets the SOW, this is the file. Nothing else needs to be read.

---

## How to read this

| Mark | Means |
|---|---|
| ✅ | Met as written. |
| ✅↷ | Met, with a mechanical difference that changes nothing — usually an added `caller` parameter Soroban requires. |
| ⚠︎ | **Deliberately different.** Argued in §B. |
| ⛔ | Not built. Argued where it appears. |

Test names are in `smart-contract/src/test/`. Run one with:

```bash
cargo test --manifest-path smart-contract/Cargo.toml f2_6_check_order
```

---

# §A · The deck, line by line

## Slide 03 · The seven objectives

| # | Deck says | | Proof |
|---|---|---|---|
| 01 | The contract holds the organisation's USDC treasury | ✅ | `treasury_tests::deposit_moves_tokens_and_the_totals_together` |
| 02 | Split into category-based virtual card balances, each with its own limit | ✅ | `cards_tests::a_new_card_starts_empty_active_and_in_a_fresh_window` · `f2_1_category_is_a_closed_set` |
| 03 | Enforce the allocation rule and the solvency rule at the contract | ✅ | `f2_2_allocation_respects_unallocated` · `invariant_one_solvency` |
| 04 | Record settlement and reconciliation for completed card payments | ✅ | `f3_4_reconciliation_links_and_is_unique` |
| 05 | **The contract is the source of truth for balances and limits** | ✅ | The gateway reads; only the contract writes. `f2_3_limit_and_balance_are_independent` |
| 06 | **Realtime authorization runs off-chain against on-chain constraints, then reconciles back** | ✅ | The whole shape of [05](05-spend-authorization.md). `two_approvals_can_exceed_the_card_and_settlement_is_the_arbiter` |
| 07 | Asset in scope: native USDC on Stellar. Contract layer only | ✅ | 7 decimals read from the token: `f1_3_deposit_usdc` · `the_contract_takes_its_decimals_from_the_token` |

## Slide 04 · Scope boundary

**In scope** — D1, D2, D3 all shipped. See below.

**Out of scope**, and nothing in this repository pretends otherwise:

| Excluded | Status |
|---|---|
| Frontend and backend application layer | ⛔ none exists |
| Card issuer / processor integration | ⛔ |
| Third-party stablecoin → fiat conversion | ⛔ |
| Security audit | ⛔ — and the reason mainnet is deferred |
| KYC / AML and compliance workflow | ⛔ |
| Multi-organisation tenancy | ⛔ — `f1_1_each_organisation_gets_an_isolated_instance` proves the isolation is structural, not policy |
| Price oracle | ⛔ — there is no price anywhere in the contract |
| Mainnet deployment | ⛔ — stated in [00](00-overview.md), [11](11-deployment.md) and the README |

**The contradiction the deck flags** — §4.1 excludes the frontend; §6 requires evidence reviewable with *minimal technical expertise*. The deck's resolution is a thin demo console (M18). Ours is different; see §B-6.

## Slide 06 · Deliverable 1 — On-Chain Treasury

| ID | Feature | | Proof |
|---|---|---|---|
| F1.1 | One instance per organisation, shared wasm, no shared state | ✅ | `f1_1_each_organisation_gets_an_isolated_instance` |
| F1.2 | Initialize: admin, operators, token, empty whitelist. **Idempotent — a second call refuses** | ✅↷ | `f1_2_initialize_runs_once` · §B-1 |
| F1.3 | Deposit USDC with the sender's authorization; correct token only; `amount > 0` | ✅ | `f1_3_deposit_usdc` — and "correct token only" is true by construction: there is no token parameter |
| F1.4 | Available balance: three numbers — deposited, allocated, unallocated | ✅ | `f1_4_available_balance_is_three_numbers` |
| F1.5 | Deposit history: an event per deposit, queryable in order | ✅↷ | `f1_5_deposit_history_is_reconstructible` · §B-7 |
| F1.6 | Role management; **never leave the contract without an admin** | ✅ | `f1_6_roles_cannot_be_emptied_of_admins` · `roles_tests::the_last_admin_cannot_be_revoked` |

The deck's Soroban note — 7 decimals, `i128` smallest units, checked subtraction, never floats — is decision #05 and is met: `decimals_tests.rs`, and `overflow-checks = true` in the release profile.

## Slide 07 · Deliverable 2 — Cards & Spending Control

| ID | Feature | | Proof |
|---|---|---|---|
| F2.1 | Cards by category · SaaS AI Advertising Cloud Other. **Enum, not free text** | ✅ | `f2_1_category_is_a_closed_set` |
| F2.2 | Allocate treasury → card; refuse past `unallocated` | ✅ | `f2_2_allocation_respects_unallocated` · `t6_over_allocation` |
| F2.3 | Set / update the spending limit. **Limit is separate from balance** | ✅ | `f2_3_limit_and_balance_are_independent` · `limits_tests::a_card_can_be_rich_and_out_of_limit` |
| F2.4 | Rebalance between cards; the total does not change; both must exist | ✅ | `f2_4_rebalance_preserves_the_total` |
| F2.5 | Card status Active ⇄ Frozen; frozen refuses **at the first check** | ✅ | `f2_5_frozen_declines_at_step_one` · `t2_frozen_card` |
| F2.6 | Validation entry point, four checks **in order**: status → limit → balance → solvency | ✅ | `f2_6_check_order_is_status_limit_balance_solvency` — all four failing at once, reasons walked back one at a time |
| F2.7 | Record the result on chain, approved / declined with a reason code | ✅ | `f2_7_decisions_are_recorded_with_a_reason` · §B-2 · §B-5 |
| F2.8 | Query: card list, card detail, spend history per card | ✅ | `f2_8_queries_for_cards_and_history` |

**The deck's "do not merge these two concepts" callout** — balance versus limit — is honoured at every level: separate fields, separate gates, separate reason codes, separate columns in the reconciliation report.

**Invariant number one** — `Σ card.balance + unallocated == the USDC the contract holds` — see §B-3 for the refinement, `invariant_one_solvency` for the test, and `harness::F::check_books` for the assertion every one of the 113 tests ends with.

## Slide 08 · Deliverable 3 — Withdrawal & Reconciliation

| ID | Feature | | Proof |
|---|---|---|---|
| F3.1 | Settlement whitelist, admin only. Removing an address does not affect old records | ✅ | `f3_1_settlement_whitelist` · `settle_tests::removing_an_address_leaves_past_settlements_alone` |
| F3.2 | Withdraw & settle: debit **the originating card**, transfer USDC out | ✅ | `f3_2_settlement_debits_the_originating_card` — asserts the other cards and the pool are untouched |
| F3.3 | Three guards: authorized operator · whitelisted address · at most the recorded spend | ✅ | `f3_3_three_guards` — and five more, see [06](06-settlement-and-reconciliation.md) |
| F3.4 | Reconciliation record linking settlement to spend via `spend_id`; never settle twice | ✅ | `f3_4_reconciliation_links_and_is_unique` · `t10_settlement_amount_and_double_settle` |
| F3.5 | Reconciliation ledger query, exportable as a table | ✅ | `f3_5_reconciliation_ledger_query` · `smart-contract/.deploy/reconciliation.md` |

**"Settlement address is a placeholder whitelisted account; no fiat conversion; no issuer integration. Do not build anything more around this."** Met literally — nothing was.

**Invariant number two** — `amount ≤ spend.amount`, `spend.status == Approved`, `spend.settled == false` before executing. All three are gates 5, 3 and 4. `invariant_two_no_double_settlement`.

## Slide 09 · Data model

| Struct | Deck's fields | | Note |
|---|---|---|---|
| `Config` | admin · token · initialized | ✅↷ | `founder` · `token` · `decimals`. `initialized` is the key's existence; `decimals` is read from the token — §B-8 |
| `Roles` | `Map<Address, Role>` | ✅ | instance storage |
| `Treasury` | total_deposited · total_allocated · total_settled | ✅ | exactly |
| `Card` | id · category · label · balance · limit · limit_window · spent_in_window · status | ✅ | `limit_window` is `limit_window_secs` plus `window_start` — decision #02 |
| `SpendRecord` | id · card_id · amount · merchant_ref · decision · reason_code · settled | ✅↷ | plus `ledger_seq` and `timestamp` — required by the deck's own UX principle 05 |
| `Settlement` | id · spend_id · card_id · amount · to_address | ✅↷ | plus `ledger_seq` and `timestamp` |
| `Whitelist` | `Map<Address, bool>` | ✅↷ | `Map<Address, ()>` — it is a set; membership is the whole fact |

Storage classes are as the deck specifies: `Config` `Roles` `Treasury` `Whitelist` in instance, `Card` `SpendRecord` `Settlement` in persistent.

**TTL & archival** — decision #08, resolved as *both*. See §B-4.

**"Events are infrastructure, not decoration"** — met, and taken further than the deck asks: events carry whole structs rather than deltas, and `rebuild_test.rs` proves the stream alone reproduces the state.

## Slide 10 · The API surface

Every entry point the deck freezes, and where it is.

| Deck | | Shipped as |
|---|---|---|
| `initialize(admin, token, operators)` | ✅↷ | plus `__constructor` with the same signature — §B-1 |
| `grant_role(addr, role)` / `revoke_role(addr)` | ✅↷ | `caller` first |
| `deposit(from, amount)` | ✅ | exactly |
| `create_card(category, label, limit)` | ✅↷ | `caller` first, `window_secs` last — decision #02 |
| `allocate(card_id, amount)` | ✅↷ | `caller` first |
| `deallocate(card_id, amount)` | ✅↷ | |
| `transfer_between_cards(from, to, amount)` | ✅↷ | |
| `set_limit(card_id, limit)` | ✅↷ | plus `window_secs` |
| `set_card_status(card_id, status)` | ✅↷ | admin **or** operator, as the deck's own table says |
| `authorize_spend(card_id, amount, ref) → Decision` | ✅↷ | returns the whole `SpendRecord`, which contains the decision and more |
| `add_whitelist(addr)` / `remove_whitelist(addr)` | ✅↷ | |
| `settle(spend_id, to, amount)` | ✅↷ | |
| `get_treasury()` · `list_cards()` · `get_card(id)` | ✅ | plus `get_unallocated`, `get_card_live`, `get_remaining_limit` |
| `list_spends(card_id)` · `get_settlement(id)` | ✅ | plus `get_spend`, `list_settlements`, `get_settlement_of_spend` |

**The `caller` parameter** is the ✅↷ throughout. Soroban needs an address to call `require_auth` on. It is mechanical and changes nothing about who may do what.

## Slides 11–14 · The flows

| Flow | | Note |
|---|---|---|
| Deposit | ✅ | `deposit(from, …)` takes `from` rather than reading the caller, exactly as the deck's figcaption explains |
| Allocation | ✅ | Guard: `amount ≤ unallocated` **and** caller is admin. Refuses with `OverAllocation` / `Unauthorized` |
| Spend authorization | ✅ | Two lanes as drawn. The race window the deck marks in brass is decision #01 — accepted, see §B |
| Settlement & reconciliation | ✅⚠︎ | The guards and the record are as drawn. The deck draws declines flowing into a `panic` box — §B-2 |

## Slide 15 · State machines

**Card status** — `Active ⇄ Frozen`, and a frozen card returns `CARD_FROZEN`. ✅ `f2_5_frozen_declines_at_step_one`.

**Spend lifecycle** — `Pending → Approved → Settled → Reconciled`, with `Declined` branching off.

Shipped slightly differently, and worth being precise about:

| Deck state | Where it lives |
|---|---|
| `Pending` | Not modelled. `authorize_spend` decides in one call; there is no state between arriving and being decided |
| `Approved` / `Declined` | `SpendRecord.decision` |
| `Settled` | `SpendRecord.settled` — a flag rather than a state, so it composes with a future `Captured` |
| `Reconciled` | **Off-chain, by the indexer.** The contract cannot look at its own transaction after the fact — see [06](06-settlement-and-reconciliation.md) |

**The deck's own warning** — that a real network has authorization-then-capture, partial capture, void, refund and chargeback, and that `SpendRecord` should leave room rather than hard-code two states — is met: `Decision` is an enum, and `settled` is a separate flag. `spend_records_are_extensible_beyond_two_states`.

## Slide 16 · UI/UX

**Track A — the demo console.** ⛔ Not built. §B-6.

**Track B — the product dashboard.** The deck says draw it, do not build it. Not built. Its purpose — checking that the contract API can serve a real screen — is served instead by `smart-contract/.deploy/reconciliation.md`, which is built from the API and renders every figure the wireframes show.

The five UX principles constrain the contract, and that is where they are honoured:

| # | Principle | | Proof |
|---|---|---|---|
| 01 | Every figure traces to a transaction hash | ✅ | Every row in `evidence.md` and `reconciliation.md` links to Stellar Expert |
| 02 | A decline states its reason, not "failed" | ✅ | `ux_02_every_decline_carries_a_distinct_reason` · §B-5 |
| 03 | Guardrails visible before submitting | ✅ | `ux_03_headroom_is_readable_before_a_spend` · `ux_03_the_live_view_rolls_an_expired_window` |
| 04 | Balance and limit are two separate bars | ✅ | Separate fields, gates, reason codes, and columns in the report |
| 05 | Off-chain and on-chain state are separate moments | ✅ | `ux_05_records_are_stamped_with_their_ledger` |

The deck's stated consequence — *"principles 02 and 05 force the contract to return a `reason_code` and store a `ledger_seq`"* — is exactly what shipped.

## Slides 17–18 · Wireframes

Not built (Track A/B above). Every figure they show is produced by `backend/indexer report`: the three treasury numbers, the per-card balance and limit bars, the spend history with reason codes, and the reconciliation table with its "2 of 3 settled" count.

The wireframes' example figures — 250,000 deposited, 182,500 allocated, 67,500 unallocated, a 42,300 SaaS card, an AI card at 37,000 of a 50,000 limit declining 18,900 — are the figures `demo.sh` and `demo_replay.rs` actually drive.

## Slides 19–20 · The nineteen modules

| # | Module | | Definition of done met by |
|---|---|---|---|
| M1 | `core::config` | ✅ | `config_tests.rs` — 3 tests |
| M2 | `core::roles` | ✅ | `roles_tests.rs` — 9 tests, including the last-admin guard |
| M3 | `treasury::deposit` | ✅ | `treasury_tests.rs` |
| M4 | `treasury::accounting` | ✅ | `check_books` after all 113 tests |
| M5 | `cards::registry` | ✅ | `cards_tests.rs` — monotonic ids, enum category |
| M6 | `cards::allocation` | ✅ | `cards_tests.rs` · `t6_over_allocation` |
| M7 | `cards::limits` | ✅ | `limits_tests.rs` — 11 tests |
| M8 | `cards::status` | ✅ | `cards_tests.rs` · `f2_5_frozen_declines_at_step_one` |
| M9 | `spend::authorize` | ✅ | `spend_tests.rs` — 9 tests |
| M10 | `settle::whitelist` | ✅ | `settle_tests.rs` · `t9_…` |
| M11 | `settle::withdraw` | ✅ | `settle_tests.rs` — every guard has a negative test |
| M12 | `settle::reconciliation` | ✅ | `settle_tests.rs` · `t10_…` |
| M13 | `events` | ✅ | **`rebuild_test.rs`** — the stream alone reproduces the state |
| M14 | `tests::unit` | ✅ | 113 tests · 96.5% line coverage |
| M15 | `tests::integration` | ✅ | `acceptance_tests.rs` — all four SOW case groups |
| M16 | `rpc::indexer` | ✅ | `backend/` — 18 tests, plus asserted against a live network by `demo.sh` |
| M17 | `deploy::scripts` | ✅ | `scripts/deploy.sh` · `scripts/demo.sh` — one command each |
| M18 | `demo::console` | ⛔ | **Not built.** §B-6 — and the deck marks it *Should*, the only cuttable module |
| M19 | `docs::evidence` | ✅ | this set · `README.md` · `smart-contract/.deploy/evidence.md` |

**"Do not cut M15 or M19."** Neither was.

**"If something must be cut, M18 is the only candidate — but then it must be replaced by a narrated video demo, or the Chapter Lead cannot verify it themselves."** That is precisely what happened, and §B-6 argues the replacement is stronger than the console would have been.

## Slide 22 · The test matrix

All ten, one test each, named for the case. See [10-testing.md](10-testing.md) for the table.

**"After each case, assert the solvency invariant again."** Every test in the suite ends with `check_books()`, which asserts more than the deck asks: it sums the actual cards and compares the token's real balance, neither of which the contract itself can check.

## Slide 24 · Evidence of completion

| Deliverable | Deck asks for | Where it is |
|---|---|---|
| D1 | GitHub repo · deployed contract link · transaction link · test results · demo | `README.md` evidence table · `smart-contract/.deploy/evidence.md` rows 1–11 · `coverage.txt` |
| D2 | Contract transaction links · screenshot · test results · demo | rows 12–21, including three **recorded declines** with hashes |
| D3 | Settlement transaction hash · explorer link · reconciliation record · test results · demo | rows 22–30 · `smart-contract/.deploy/reconciliation.md` |

**"The easiest evidence to review is a single table in the README: one row per step of the end-to-end demo, each with a transaction hash linking straight to Stellar Expert, so the Chapter Lead can tick §6.2 without reading code."** That is exactly what `smart-contract/.deploy/evidence.md` is, and it is generated by the run rather than transcribed afterwards.

## Slide 25 · The nine open decisions

All nine closed before implementation. See [00-overview.md](00-overview.md) for the table and §B below for the ones that changed the code.

## Slide 26 · This week's work

| Deck says | |
|---|---|
| Close the nine decisions, at least 01, 02 and 04 | ✅ all nine |
| Freeze the API surface and commit it as an interface before implementing | ✅ `types.rs` and `errors.rs` were written and frozen first; [02](02-contract-interface.md) is the record |
| Build the test skeleton from the ten cases on day one, not at the end | ✅ tests were written alongside each module; `acceptance_tests.rs` carries all ten |
| Prepare an empty evidence table in the README, filled in stage by stage | ✅ generated by `make demo` rather than filled in by hand — which is stronger, because nobody can transcribe a hash wrongly |

---

# §B · Where we build something other than what the deck says

Nine entries. Each says what the deck says, what we build, and why.

## §B-1 · `initialize` exists, and can never run

**Deck:** `initialize(admin, token, operators)` is an entry point; calling it twice must fail.

**Built:** `__constructor` with the same signature, run by the host on every deployment — plus `initialize`, which therefore always returns `AlreadyInitialized`.

**Why.** Deploying and then initialising leaves a window, a few seconds wide and entirely sufficient, in which anybody watching the ledger can call `initialize` and name themselves admin of a contract somebody else paid to deploy. `stellar contract deploy -- <args>` closes it by making the two one transaction.

Keeping `initialize` as well means the deck's entry point exists and behaves as specified — a second call is refused, which is M1's definition of done — while the *first* call cannot be raced. `config_tests::initialize_is_always_refused_after_construction`.

Consequence: `Error::NotInitialized` (2) is unreachable. Kept anyway, because the alternative is an `unwrap` on `Config`, and a defensive `Result` that never fires is cheaper than a panic that fires once.

## §B-2 · A decline is a successful transaction, not a panic

**Deck:** the flow diagrams draw declined spends alongside `panic · CARD_FROZEN` boxes, and decision #04 proposes recording only approvals to save storage.

**Built:** `authorize_spend` returns `Ok` for every business refusal and writes a `SpendRecord` with the reason. It reverts only for `Unauthorized`, `CardNotFound`, `InvalidAmount` and `InvalidMerchantRef` — the cases where there is nothing sensible to record.

**Why.** A panic leaves no transaction. SOW §6 asks a reviewer with minimal technical expertise to open a declined spend and read why it was refused — and the deck's own Spend Simulator wireframe shows `DECLINED · LIMIT_EXCEEDED` next to a transaction hash, which is only possible if the decline *is* a transaction.

It costs persistent storage, and the deck is right that it does. The evidence requirement decides it.

`b1_declines_are_persisted` · `b2_a_decline_is_a_success_and_a_panic_is_not`.

This is the single most consequential difference in this document. It is why `scripts/demo.sh` needs two assertion helpers rather than one, and why `evidence.md` has three kinds of row rather than two.

## §B-3 · The solvency check reads the token, not the books

**Deck:** *"Σ card.balance + unallocated == the USDC balance the contract holds"*, asserted after every entry point.

**Built:** three invariants instead of one.

- **I1**, the accounting identity, asserted inside the contract after every write.
- **I2**, backing — `total_deposited − total_settled ≤ token.balance(contract)` — **consulted**, as check 4 of `authorize_spend`, never asserted.
- **I3**, no double settlement, enforced structurally by a storage key.

**Why.** On Stellar an issuer with clawback enabled can remove tokens from any holder, this contract included, with no bug on our side. Asserting the deck's equality would turn that into a panic on every write — a treasury that is merely *short* would become one that is *frozen*: nobody could deallocate, nobody could freeze a card, nobody could wind it down.

So the shortfall surfaces as `ReasonCode::TreasuryInsolvent` on a recorded, declined spend, and everything that does not need the missing money keeps working.

`b3_a_short_treasury_declines_rather_than_freezing` · `invariant_one_solvency`.

The deck's equality is still asserted — in the **test harness**, where it belongs, because a test can also see the token balance and can distinguish a clawback from a bug. It is one line of `check_books`.

## §B-4 · TTL: both halves of the answer, not one

**Deck, decision #08:** bump TTL on every write, *or* keep historical records in events and let an off-chain index hold them long-term. "Decide before day 5."

**Built:** both.

**Why.** They solve different problems. Bumping keeps alive the entries an entry point needs in order to *act* — config, roles, treasury, whitelist, cards — and losing one of those breaks the contract. Events are what the long *history* rests on, and no amount of bumping makes a record survive indefinitely.

Choosing only bumping means history is hostage to TTL. Choosing only events means an archived card cannot be spent from. The cost of doing both is one `extend_ttl` per touched key.

The bump is **bounded** — only the keys a call actually named — because an unbounded one grows with the card count until an entry point stops fitting in a transaction.

`rebuild_test.rs` is what makes the events half load-bearing rather than aspirational.

## §B-5 · Reason codes are names on chain, not numbers

**Deck:** reason codes are written as `LIMIT_EXCEEDED`, `CARD_FROZEN` and so on, without saying how they are encoded.

**Built:** a plain `#[contracttype]` enum with **no integer discriminants**, so each variant encodes as its own symbol.

**Why.** Giving the variants explicit numbers makes the SDK encode the enum as a `u32`. A reviewer opening a declined transaction then sees `reason_code: 2`, where the requirement is that they can read *why* the spend was refused.

This was found by running the acceptance script against testnet. The first version had discriminants, and step 14 failed with:

```
step 14 was declined for '1', not 'CardFrozen'.
```

which is the assertion doing its job — and a good example of why the script matches reason codes exactly rather than checking that *something* was declined.

`ux_02_every_decline_carries_a_distinct_reason`.

## §B-6 · No demo console. A generated evidence package instead

**Deck:** module M18 — a five-screen read-only console on testnet, marked *Should*, and the deck's proposed resolution of the §4.1/§6 contradiction.

**Built:** nothing. Instead:

| | |
|---|---|
| `make demo` | 30 steps on testnet, one command, writing `evidence.md` with a hash per step |
| `backend/indexer report` | the same treasury rebuilt **independently from its event stream**, as a table in whole USDC |
| `coverage.txt` | what the suite actually reaches |
| a narrated video | walking through the above |

**Why.** The deck names this the only cuttable module and says the replacement must let a Chapter Lead verify things themselves. The argument for the replacement being *stronger* than the console, not merely acceptable:

- A console **displays what it read**. If it had a bug, it would display the bug convincingly. The evidence file contains transaction hashes a reviewer opens on a third-party explorer that has no connection to this repository.
- Every refusal in the run is **asserted against its exact code**, and a mismatch stops the run and produces no file. A console has no equivalent — it shows a state and cannot fail.
- The reconciliation table is built by a **second, independent implementation** reading the event stream rather than the view functions. When the two agree, that is corroboration; a console reading the same view functions would only be a second rendering of the same claim.
- A console would have been Track A, and §4.1 excludes frontends. The evidence package is unambiguously tooling.

The honest cost: there is no click-through experience. A reviewer reads a markdown table and opens links. That is a worse demo and better evidence, and §6 asks for evidence.

## §B-7 · Deposit history lives in events, not in a stored list

**Deck, F1.5:** *"an event per deposit, plus a query returning them in order."*

**Built:** the event, and the resulting `Treasury` on it. No stored list and no `list_deposits` view.

**Why.** A stored list grows without bound in instance storage — which is loaded on *every* invocation, so every call would pay for the whole deposit history forever. That is the one storage mistake in this contract that gets worse over time rather than staying constant.

The query is served by `backend/indexer report`, which renders deposits with their transaction hashes — which the contract could not have provided anyway, since it cannot see its own transaction hash.

This is the same reasoning as §B-4, applied to one feature: the contract holds what it needs to *act*; the stream holds what somebody needs to *read*.

## §B-8 · `decimals` is read from the token, and `initialized` is not a field

**Deck, `Config`:** `admin · token · initialized`.

**Built:** `founder · token · decimals`.

**Why, three small things.**

`initialized` as a boolean is redundant: the existence of the `Config` key *is* the flag, and a separate field is a second source of truth that can disagree with the first.

`admin` is renamed `founder` because it is a record of who deployed, not a standing privilege. Authority comes from the role table, which the founder is seeded into and **can be revoked from** — provided somebody else holds `Admin` by then. `roles_tests::the_founder_holds_no_privilege_beyond_its_role` asserts that the field grants nothing.

`decimals` is added and read **from the token contract**, never taken from the deployer. It is used in no arithmetic — there is none that scales — but it is what an explorer and the indexer render amounts with, and a value copied from an assumption rather than read from the token would be wrong by orders of magnitude with nothing on chain to contradict it.

## §B-9 · `settle` has nine gates, not three

**Deck, F3.3:** three guards — authorized operator, whitelisted address, at most the recorded spend amount.

**Built:** those three plus six more: the destination must not be the contract itself, the spend must exist, the amount must be positive, the spend must be **approved**, it must not be settled already, and the card must still have the balance.

**Why.** Two of the additions are consequences of §B-2 and are not optional. Because declines are recorded, a declined `SpendRecord` *exists* — and if it could be settled, the contract would have refused a spend and then paid it. `SpendNotApproved` closes that.

`AlreadySettled` is invariant I3, which the deck states on slide 09 but does not list among F3.3's guards.

And re-checking the card balance is what makes the contract the final arbiter of the race window decision #01 accepts. The approval may be several ledgers old and another settlement may have drained the card since; without gate 6 the contract would over-pay.

`f3_3_three_guards` covers the deck's three; `settle_tests.rs` and `adversarial_tests.rs` cover all nine.

The destination guard was added last, after the adversarial pass found that settling to the contract's own address stranded the amount — off the card, never back into `unallocated`, and beyond the reach of every entry point. See §D.

---

# §C · Where the deck is wrong or leaves a gap

Not disagreements about approach — places where following the deck literally would produce something incorrect.

**Declines drawn as panics.** Slides 12 and 14 route refusals into `panic` boxes without distinguishing the two kinds. Building that literally makes §6's evidence requirement unsatisfiable, because there is no transaction to open. §B-2.

**The invariant as an assertion.** Slide 09 states it as an equality to assert after every entry point. Doing so makes an issuer clawback — an external event, not a bug — permanently freeze the treasury. §B-3.

**`Decision` as the return type.** Slide 10 writes `authorize_spend(...) → Decision`. A bare decision is not enough for the caller to do anything with: the gateway needs the reason, and the evidence table needs the record id. Returning the whole `SpendRecord` costs nothing and answers both.

**The limit period is never stated.** Slide 07 says "set / update spending limit" and slide 25 admits the period is undecided. Everything about `spent_in_window` and its reset depends on the answer, so it cannot be deferred past the first line of `Card`. Decision #02 makes it a per-card field, which also lets a daily advertising card sit next to a monthly SaaS card.

**`Reconciled` has no owner.** Slide 15 puts it in the spend lifecycle as though the contract sets it. The contract cannot: it has no way to look at its own transaction after the fact. It belongs to the indexer, and the split between `index` and `verify` is what keeps "we saw an event" from being mistaken for "we checked the chain". [06](06-settlement-and-reconciliation.md).

**Nothing about what happens when `getEvents` pages.** The deck treats the event stream as a given. In practice an empty first page is routine and does not mean the stream has ended — the trap that made the first working version of the indexer report an empty treasury while exiting zero. [09](09-offchain-integration.md).

---

# §D · Bugs found after the first green build, and fixed

The deck's test matrix (slide 22) and the module definitions of done were all met by the first
build that went green: 113 tests, the acceptance scenario passing on testnet, an evidence package
produced. That is what conformance to the specification looks like, and it was not enough.

A separate pass went looking for inputs the specification never mentions. It found five real bugs.
They are recorded here because the audit record should say what the contract *was* as well as what
it *is* — and because the shape of them is worth knowing before adding anything to this contract.

| # | Bug | Where it is now guarded | Test |
|---|---|---|---|
| D-1 | A limit window near `u64::MAX` was accepted, and then **every call that rolled the window trapped** — including the two view functions. The card could not spend and its limit could not be corrected, because correcting it also rolls the window. Permanent, with no upgrade path. | `cards::MAX_WINDOW_SECS`, plus `checked_add` in `roll_window` as a second line | `a_huge_limit_window_is_refused` |
| D-2 | The same, through `set_limit` — the worse door, because it takes a working card and makes it unreadable. | the same bound, checked **before** any mutation | `a_huge_window_through_set_limit_is_refused` |
| D-3 | `get_settlement` answered `SpendNotFound` (9) for a missing settlement — a claim about a different ledger, which would send somebody looking for a spend that exists. | `SettlementNotFound` (21) | `a_missing_settlement_says_so` |
| D-4 | Settling to the treasury's **own address** stranded the amount. The transfer is a no-op; the bookkeeping is not. The money left the card, never returned to `unallocated`, and became unreachable by every entry point. | `InvalidDestination` (22), refused at `add_whitelist` *and* at `settle` | `the_contract_cannot_be_its_own_settlement_destination` |
| D-5 | `mock-token` **minted** on `transfer(x, x, n)`: it read both balances, then wrote both, so the second write won. Anybody can mint this fixture, so the minting was not the harm — the harm is that `check_books` measures the contract against this token, and a fixture that inflates can hide a real accounting bug. | a self-transfer is an explicit no-op | `the_fixture_token_does_not_mint_on_a_self_transfer` |

## What this says about the specification

Four of the five are an argument at the edge of its type; the fifth is an aliasing case. None of
them is exotic, and **none of them is something the deck could reasonably have specified** — a
build brief says what a limit window is for, not that it must not overflow a `u64`.

That is the general shape: a specification says what the system should do, and the gaps are in
what it should refuse. `adversarial_tests.rs` exists to hold that second category, and the four
shapes worth trying against anything added later are:

- **the edge of the type** — `u64::MAX`, `i128::MAX`, zero, one past a stated bound;
- **an address that is one of ours** — the contract, the token, the caller;
- **aliasing** — the same id or address in two parameters;
- **the failure of something downstream** — a token that refuses the transfer *after* the writes.

## What it says about the invariants

Nothing changed. I1, I2 and I3 held throughout, and `check_books` never failed on any of the five
— which is the point worth noticing rather than a reassurance. D-1 and D-3 are not accounting
bugs at all, and D-4 broke a property no invariant states: that money is always in exactly one of
the three places the [money map](../resources/flows/09-money-map.md) names. D-5 was in the
measuring instrument.

An invariant catches what it was written to catch. It is not a substitute for trying the input
nobody thought of.
