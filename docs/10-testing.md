# 10 · Testing

The SOW puts 20% of the budget on tests and names them as a line item that cannot be cut. This is what that bought.

```
smart-contract   128 tests · 96.5% line coverage over the contract
backend          18 tests · no network required
```

## The one thing every scenario ends with

```rust
f.check_books();
```

Every test in the suite, without exception. It asserts:

```
Σ card.balance summed from the ACTUAL cards == total_allocated − total_settled
Σ card.balance + unallocated               == total_deposited − total_settled
token.balance(contract)                    == total_deposited − total_settled − clawed_back
every settled spend has exactly one settlement · no spend has two
```

### Why it exists rather than trusting the contract's own assertion

The contract asserts invariant I1 after every write, cheaply, from three running totals. Those totals are **internally consistent by construction**: if `allocate` raised `total_allocated` and forgot to credit the card, they would still agree with each other and the assertion would pass.

And no assertion inside the contract can see the token's balance. A `settle` that updated every record and never issued the transfer looks perfect from the inside — the books balance, the invariants hold, and the money is somewhere else.

`check_books` closes both gaps: it **sums the actual cards** and compares the **token's actual balance**. Those two comparisons are where a missing credit and a missing transfer respectively have to show up.

This is also what the deck asks for, in its own words (slide 22):

> After **each** case above, assert the solvency invariant again. Cheap to write, and it is the thing that catches an accounting error that no individual case sees.

## Exact codes, never `is_err()`

```rust
assert_err!(f.c.try_allocate(&f.outsider, &card, &amount), Error::Unauthorized);
```

`is_err()` would pass for any of the twenty error codes. The precision is not academic: `scripts/demo.sh` greps for `Error(Contract, #N)` by exact number and fails the run on a mismatch, the indexer branches on reason codes, and the gateway branches on both. **A test that cannot tell 6 from 8 is not testing the thing that matters.**

This is also why every entry point returns `Result` rather than using `panic_with_error!` — `try_*` on the generated client only yields `Err(Ok(Error::X))` for functions declared that way.

## The fixture

`src/test/harness.rs` gives every test an `F`: a contract, a 7-decimal token, six identities, and shorthands.

Two details in it are worth knowing about:

**The ledger does not start at zero.** A default `Env` starts at ledger 0, timestamp 0 — values the real network can never produce, and both hide bugs. A record stamped `ledger_seq: 0` is indistinguishable from one whose field was never set, and a card created at timestamp 0 has a limit window that started at the Unix epoch. The fixture starts at ledger 1,000,000 and a plausible wall clock, which makes "the field was populated" a real assertion. Three tests were passing on zeroes before this was fixed.

**`clawback` is the only honest route to `TREASURY_INSOLVENT`.** Test T5 needs a card with balance and limit to spare on a treasury that still cannot pay. The contract's own accounting makes that state unreachable — I1 guarantees the treasury covers every card — so it is only reachable when tokens leave without the contract's knowledge. `mock-token` therefore has an unauthenticated `clawback`, which is what an issuer with clawback enabled can do on Stellar. Reaching T5 by writing storage directly would prove nothing about the contract; this proves check 4 reads the token rather than its own books.

## The suite

| File | Tests | Covers |
|---|---|---|
| `config_tests.rs` | 3 | M1 — construction, and `initialize` always refusing |
| `roles_tests.rs` | 9 | M2 — grant, revoke, both role boundaries, the last-admin guard |
| `treasury_tests.rs` | 5 | M3 M4 — deposit moves tokens *and* totals; a full cycle's arithmetic |
| `cards_tests.rs` | 12 | M5 M6 M8 — registry, allocation, rebalancing, status |
| `limits_tests.rs` | 11 | M7 — the window, its lazy roll, and every way a limit changes |
| `spend_tests.rs` | 9 | M9 — the four checks, the frozen order, the race window |
| `settle_tests.rs` | 13 | M10 M11 M12 — the whitelist, the nine gates, the ledger |
| `acceptance_tests.rs` | 11 | **T1–T10**, the deck's matrix, one test each |
| `adversarial_tests.rs` | 15 | inputs the specification never mentions — **five real bugs came from here** |
| `deck_conformance.rs` | 30 | the deck line by line — F1.1…F3.5, the invariants, the UX principles, the deviations |
| `decimals_tests.rs` | 5 | smallest units, `i128` extremes, guarded subtraction, boundary inclusivity |
| `property_tests.rs` | 2 | randomised operation orders, books checked after every step |
| `rebuild_test.rs` | 2 | M13 — the event stream alone reproduces contract state |
| `demo_replay.rs` | 1 | the acceptance scenario, in process |

`acceptance_tests.rs` and `deck_conformance.rs` duplicate coverage that exists elsewhere, on purpose. The other files are organised by **how the contract works**; those two are organised by **what was promised**, and those are different questions with different failure modes. A refactor that keeps every behavioural test green while dropping a deck requirement is exactly what they catch.

## T1–T10 — the deck's matrix

Slide 22, one test each, named for the case so a reviewer holding the deck can find the proof of any line without reading the contract.

| # | Situation | Expected | Test |
|---|---|---|---|
| T1 | Active card, under limit, sufficient balance | `Approved` | `t1_approved_spend` |
| T2 | Card frozen | `Declined · CardFrozen` | `t2_frozen_card` |
| T3 | Over the remaining window limit | `Declined · LimitExceeded` | `t3_over_limit` |
| T4 | Limit fine, card balance short | `Declined · InsufficientCardBalance` | `t4_insufficient_card_balance` |
| T5 | Card fine, treasury cannot pay | `Declined · TreasuryInsolvent` | `t5_treasury_insolvent` |
| T6 | Allocate past unallocated | `Err(OverAllocation)` | `t6_over_allocation` |
| T7 | Non-admin calls `allocate` | `Err(Unauthorized)` | `t7_allocate_requires_admin` |
| T8 | Non-operator calls `settle` | `Err(Unauthorized)` | `t8_settle_requires_operator` |
| T9 | Settle to a non-whitelisted address | `Err(NotWhitelisted)` | `t9_settlement_address_must_be_whitelisted` |
| T10 | Settle over the spend, and settle twice | `Err(AmountExceedsSpend)` · `Err(AlreadySettled)` | `t10_settlement_amount_and_double_settle` |

T3 uses the deck's own figures from the Spend Simulator wireframe: a card with 37,000 of a 50,000 limit spent, asked for 18,900 against the 13,000 left.

The four groups the SOW names — approved spend, declined spend, over-limit, over-allocation, role permission — are also driven end to end in one scenario by `the_four_sow_case_groups_in_one_pass`, with the books checked after each.

### The adversarial pass

`adversarial_tests.rs` was written the other way round from every other file here: not from the
specification, but by looking for inputs the specification never mentions. **It found five real
bugs**, and each of the tests that found one is kept so it cannot come back.

| Test | The bug |
|---|---|
| `a_huge_limit_window_is_refused` | `create_card(…, u64::MAX)` was accepted, then **trapped on every read of the card** — `authorize_spend`, `set_limit`, `get_card_live`, `get_remaining_limit`. The card could not spend and its limit could not be corrected, because correcting it also rolls the window. Permanent, with no upgrade path. |
| `a_huge_window_through_set_limit_is_refused` | the same bug through a second door, on a card that was already working |
| `a_missing_settlement_says_so` | `get_settlement` answered `SpendNotFound` — a claim about a different ledger |
| `the_contract_cannot_be_its_own_settlement_destination` | settling to the treasury's own address stranded the amount: off the card, never back to `unallocated`, unreachable by any entry point |
| `the_fixture_token_does_not_mint_on_a_self_transfer` | `mock-token` **created tokens** on `transfer(x, x, n)` — and it is the token `check_books` measures the contract against, so a fixture that inflates can hide a real accounting bug |

Nothing here is exotic. Four of the five are an argument at the edge of its type — `u64::MAX`, an
address that happens to be the contract's own, a 65th byte — and the fifth is an aliasing case
(`from == to`) that only appears when two names refer to one thing. Those are the shapes worth
trying first against anything new.

Two more tests in that file assert a property that was already true and whose failure would have
been silent: `a_failed_deposit_transfer_takes_the_books_with_it` and
`a_failed_settlement_transfer_takes_everything_with_it`. Both `deposit` and `settle` write state
and *then* transfer (step 9 of the discipline). If the host did not revert those writes when a
transfer fails, the books would be permanently wrong with no bug anywhere in this contract. It
does revert, and now that is checked rather than assumed.

## Where the interesting tests are

### The check order

`spend_tests.rs::the_check_order_decides_the_reason_when_several_fail` and `deck_conformance.rs::f2_6_check_order_is_status_limit_balance_solvency`.

A card is set up failing **all four** checks at once — frozen, over limit, over balance, and on a clawed-back treasury — and the reasons are walked back one at a time by fixing each condition. This is the only way to pin an ordering down; testing each check in isolation says nothing about which wins.

### The race window

`spend_tests.rs::two_approvals_can_exceed_the_card_and_settlement_is_the_arbiter`.

Two 40-unit approvals against a 60-unit card. Both approved. The first settlement succeeds; the second is refused with `InsufficientCardBalance`, and the orphan remains visible as an approved, unsettled record.

This test exists so decision #01 cannot be undone by accident. If a future change introduces an on-chain hold, this test fails and somebody has to decide it deliberately.

### The rebuild — M13's definition of done

`rebuild_test.rs::the_event_stream_alone_reproduces_the_contract_state`.

A scenario touching every mutation, with events folded **as they are published**, then compared against the contract's own view functions: every card, every spend (including the `settled` flag), every settlement, and the whitelist.

Two details make it a real test rather than a shape check:

- **Events are absorbed after each call, not read in bulk at the end.** `Env::events().all()` returns what the *most recent* invocation published, so a fold written at the end would silently see one transaction's worth and pass on a subset.
- **The whitelist covers add-then-remove.** An address added and then removed must rebuild as removed — the case a consumer handling only additions gets wrong, and the one that decides whether a settlement destination looks authorised.

`replaying_the_stream_twice_changes_nothing` covers the other half: the fold is idempotent, which is what makes an at-least-once RPC feed safe.

### Randomised orders

`property_tests.rs::the_books_balance_after_any_sequence_of_operations`.

Five seeds × 200 operations against four cards, with `check_books()` after **every single step**.

Nothing asserts an outcome. Any individual call may legitimately be refused, and being refused is as interesting as succeeding. The assertion is that **whatever happens, the books balance afterwards** — and accounting bugs live in orderings rather than in operations, which is the one thing a hand-written scenario cannot enumerate.

No `proptest`: it does not build for a `no_std` Soroban crate, and `Math::random` has no meaning in a deterministic host. A small seeded xorshift gives the thing that matters — an ordering nobody chose — reproduced exactly on every run and every machine. A failure anyone can reproduce by running the suite is worth more than an unreproducible one found faster.

Each seed must produce at least one settlement, asserted, or the test is a very slow no-op.

### The acceptance scenario, twice

`demo_replay.rs::the_acceptance_scenario` is `scripts/demo.sh` in process: same sequence, same order, same expected outcomes, under a second.

That is where a broken scenario should fail — not on testnet, after several minutes of funded transactions, with a partial run already on chain. **The two must be kept in step**; the step numbers in the test are the step numbers the script prints and the rows the evidence table carries.

## Coverage

```bash
make coverage      # → smart-contract/.deploy/coverage.txt
```

```
lib.rs        98.40%
storage.rs    98.54%
cards.rs      97.37%
roles.rs     100.00%
settle.rs    100.00%
spend.rs     100.00%
events.rs    100.00%
invariant.rs  70.00%
types.rs       0.00%
TOTAL         96.52%
```

Test modules and `mock-token` are excluded from the measurement. Counting them inflates the figure with code that is by definition executed, and the question a reviewer is asking is how much of the **contract** the suite reaches.

Two figures need explaining rather than defending:

**`invariant.rs` at 70%** — the uncovered lines are the panic branches of `assert_books`. They are unreachable by design: reaching one means the contract's own arithmetic is wrong. A test could reach them by corrupting storage directly, which would test the assertion rather than the contract.

**`types.rs` at 0%** — derive-generated conversion code with no logic of its own. Every field in it is exercised through the entry points; none of it has a branch to take.

A passing suite says nothing about what it never ran, which is why this file goes in the evidence package alongside the test log.

## The acceptance scenario, on chain

```bash
KEY=sx-deployer make demo
```

Thirty steps, on testnet, writing `smart-contract/.deploy/evidence.md` with a hash per step. See [11-deployment.md](11-deployment.md).

**The property that makes it evidence rather than theatre:** every refusal asserts its exact code and fails the script on a mismatch. Without that, a step failing for the *wrong* reason still passes — the script goes green, the evidence file fills up, and nobody has verified anything. That is worse than no evidence, because it looks like evidence.

Two assertion helpers, because this contract has two families of refusal:

```bash
call 6            "allocate past the pool"  -- …   # expects Error(Contract, #6). No transaction.
call_decision Declined:LimitExceeded "…"    -- …   # expects a SUCCESSFUL tx carrying that reason
```

And the off-chain half is asserted rather than trusted:

```bash
expect_eq "settlements the indexer rebuilt" "$(jq '.settlements | length' "$IDX")" "2"
```

An indexer that read nothing exits zero and writes a well-formed state file full of zeroes. Those assertions compare two independent views of the same treasury — the contract's, and the one folded from events — and agreement between them is the whole claim M13 makes. They were added after a paging bug did exactly that during development and the script went green anyway.
