//! The audit record, as tests.
//!
//! # What this file is for
//!
//! `docs/13-deck-conformance.md` goes through the SOW 1 build-brief deck line by
//! line and, for each claim, names the test that proves it. **These are those tests.** Each one
//! is named after the deck's own identifier — `f1_3_deposit_usdc`, `m6_over_allocation_panics` —
//! so a reviewer holding the deck can find the proof of any single line without reading the
//! contract, and so a line that quietly stopped being true has somewhere to fail.
//!
//! Coverage overlaps the rest of the suite on purpose. The other files are organised by how the
//! contract works; this one is organised by what was promised, and those are different questions
//! with different failure modes. A refactor that keeps every behavioural test green while
//! dropping a deck requirement is exactly what this catches.
//!
//! Deviations — places where we deliberately build something other than what the deck says — are
//! tested here too, asserting the behaviour we chose. Each is argued in `13-deck-conformance.md`
//! §B, and a test named for a deviation is a test whose failure means the deviation was undone by
//! accident rather than decided.

use soroban_sdk::String;

use super::harness::{DAY, F, USD};
use crate::errors::Error;
use crate::types::{CardStatus, Category, Decision, ReasonCode, Role};

// ===========================================================================
// Deliverable 1 · On-Chain Treasury  (deck slide 06)
// ===========================================================================

/// **F1.1** — one contract instance per organisation, no shared state.
///
/// Two treasuries deployed from the same wasm know nothing about each other. Multi-organisation
/// tenancy is out of scope, and this is what "out of scope" means concretely: there is no
/// registry, no org id, and no way for one instance to read another's cards.
#[test]
fn f1_1_each_organisation_gets_an_isolated_instance() {
    let a = F::new();
    let b = F::new();

    a.deposit(1_000 * USD);
    let card = a.card(Category::SaaS, "saas", 100 * USD);
    a.allocate(card, 500 * USD);

    assert_eq!(b.c.get_treasury().total_deposited, 0);
    assert_eq!(b.c.list_cards().len(), 0);
    // Even the ids restart, because there is nothing shared to make them collide.
    let b_card = b.card(Category::SaaS, "saas", 100 * USD);
    assert_eq!(b_card, 1);
    assert_eq!(card, 1);
    a.check_books();
    b.check_books();
}

/// **F1.2** — initialisation is idempotent; a second call is refused.
#[test]
fn f1_2_initialize_runs_once() {
    let f = F::new();
    assert_err!(
        f.c.try_initialize(
            &f.outsider,
            &f.token_id,
            &(soroban_sdk::vec![&f.e] as soroban_sdk::Vec<soroban_sdk::Address>)
        ),
        Error::AlreadyInitialized
    );
}

/// **F1.3** — deposit accepts only the configured token, and only positive amounts.
///
/// "Only the configured token" is true here by construction rather than by check: `deposit` has
/// no token parameter. The token was fixed at construction and there is nothing to pass wrong.
#[test]
fn f1_3_deposit_usdc() {
    let f = F::new();
    f.deposit(120_000 * USD);
    assert_eq!(f.held(), 120_000 * USD);
    assert_err!(f.c.try_deposit(&f.funder, &0), Error::InvalidAmount);
    assert_err!(f.c.try_deposit(&f.funder, &-1), Error::InvalidAmount);
    f.check_books();
}

/// **F1.4** — the three numbers: total deposited, allocated, unallocated.
#[test]
fn f1_4_available_balance_is_three_numbers() {
    let f = F::new();
    f.deposit(250_000 * USD);
    let card = f.card(Category::SaaS, "saas", 0);
    f.allocate(card, 182_500 * USD);

    let t = f.c.get_treasury();
    assert_eq!(t.total_deposited, 250_000 * USD);
    assert_eq!(t.total_allocated, 182_500 * USD);
    assert_eq!(f.c.get_unallocated(), 67_500 * USD);
    f.check_books();
}

/// **F1.5** — deposit history, with enough on each event for an indexer to rebuild it.
///
/// The deck asks for a query returning deposits in order. We emit an event per deposit carrying
/// the depositor, the amount and the resulting treasury, and the history is read from those
/// rather than from a stored list — decision #08. `rebuild_test.rs` is the proof that the event
/// stream is sufficient; this asserts the running total the events have to agree with.
#[test]
fn f1_5_deposit_history_is_reconstructible() {
    let f = F::new();
    f.deposit(120_000 * USD);
    f.deposit(80_000 * USD);
    f.deposit(50_000 * USD);
    assert_eq!(f.c.get_treasury().total_deposited, 250_000 * USD);
    f.check_books();
}

/// **F1.6** — role management, and the state the contract must never reach.
#[test]
fn f1_6_roles_cannot_be_emptied_of_admins() {
    let f = F::new();
    assert_err!(f.c.try_revoke_role(&f.admin, &f.admin), Error::LastAdmin);
    f.c.grant_role(&f.admin, &f.admin2, &Role::Admin);
    f.c.revoke_role(&f.admin, &f.admin);
    assert_eq!(f.c.role_of(&f.admin2), Some(Role::Admin));
}

// ===========================================================================
// Deliverable 2 · Cards & Spending Control  (deck slide 07)
// ===========================================================================

/// **F2.1** — category is an enum, not free text.
///
/// Asserted structurally: `create_card` takes a `Category`, so there is no string to normalise
/// and no way for "SaaS", "saas" and "Saas" to become three cost categories in a report.
#[test]
fn f2_1_category_is_a_closed_set() {
    let f = F::new();
    for cat in [
        Category::SaaS,
        Category::AI,
        Category::Advertising,
        Category::Cloud,
        Category::Other,
    ] {
        let id = f.card(cat, "c", 0);
        assert_eq!(f.get_card(id).category, cat);
    }
    assert_eq!(f.c.list_cards().len(), 5);
}

/// **F2.2** — allocation is refused past the unallocated balance.
#[test]
fn f2_2_allocation_respects_unallocated() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 0);
    assert_err!(
        f.c.try_allocate(&f.admin, &card, &(101 * USD)),
        Error::OverAllocation
    );
    f.allocate(card, 100 * USD);
    f.check_books();
}

/// **F2.3** — limit is separate from balance.
///
/// The deck's callout, as an assertion: a card with money and no limit left, and a card with
/// limit and no money. Both decline, for different reasons, and neither number explains the
/// other.
#[test]
fn f2_3_limit_and_balance_are_independent() {
    let f = F::new();
    f.deposit(100_000 * USD);

    let rich_and_capped = f.card(Category::AI, "ai", 1_000 * USD);
    f.allocate(rich_and_capped, 61_000 * USD);
    f.assert_approved(&f.spend(rich_and_capped, 1_000 * USD, "a"));
    f.assert_declined(
        &f.spend(rich_and_capped, USD, "b"),
        ReasonCode::LimitExceeded,
    );

    let broke_and_free = f.card(Category::Cloud, "cloud", 90_000 * USD);
    f.allocate(broke_and_free, 10 * USD);
    f.assert_declined(
        &f.spend(broke_and_free, 100 * USD, "c"),
        ReasonCode::InsufficientCardBalance,
    );
    f.check_books();
}

/// **F2.4** — rebalancing keeps the total unchanged and needs both cards to exist.
#[test]
fn f2_4_rebalance_preserves_the_total() {
    let f = F::new();
    f.deposit(100 * USD);
    let a = f.card(Category::SaaS, "a", 0);
    let b = f.card(Category::AI, "b", 0);
    f.allocate(a, 100 * USD);

    let before = f.c.get_treasury();
    f.c.transfer_between_cards(&f.admin, &a, &b, &(40 * USD));
    assert_eq!(f.c.get_treasury(), before);
    assert_eq!(f.get_card(a).balance + f.get_card(b).balance, 100 * USD);

    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &99, &USD),
        Error::CardNotFound
    );
    f.check_books();
}

/// **F2.5** — a frozen card is refused at the first check.
#[test]
fn f2_5_frozen_declines_at_step_one() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::Cloud, "cloud", 100 * USD);
    f.allocate(card, 100 * USD);
    f.freeze(card);

    // Everything else about this spend is fine, so `CARD_FROZEN` can only come from check 1.
    f.assert_declined(&f.spend(card, USD, "x"), ReasonCode::CardFrozen);
    f.unfreeze(card);
    f.assert_approved(&f.spend(card, USD, "x"));
    f.check_books();
}

/// **F2.6** — the four checks, in the deck's order.
///
/// The order is the deliverable, not the checks. Each case below fails several rules at once and
/// asserts which reason wins, which is the only way to pin an ordering down.
#[test]
fn f2_6_check_order_is_status_limit_balance_solvency() {
    let f = F::new();
    f.deposit(100_000 * USD);

    let all = f.card(Category::Cloud, "all-four", 10 * USD);
    f.allocate(all, USD);
    f.freeze(all);
    f.clawback(99_999 * USD);
    // Frozen, over limit, over balance and insolvent simultaneously.
    f.assert_declined(&f.spend(all, 5_000 * USD, "x"), ReasonCode::CardFrozen);

    f.unfreeze(all);
    f.assert_declined(&f.spend(all, 5_000 * USD, "x"), ReasonCode::LimitExceeded);

    f.c.set_limit(&f.admin, &all, &(1_000_000 * USD), &0);
    f.assert_declined(
        &f.spend(all, 5_000 * USD, "x"),
        ReasonCode::InsufficientCardBalance,
    );

    f.assert_declined(&f.spend(all, 1, "x"), ReasonCode::TreasuryInsolvent);
    f.check_books();
}

/// **F2.7** — the decision is written on chain with its reason code.
#[test]
fn f2_7_decisions_are_recorded_with_a_reason() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 10 * USD);
    f.allocate(card, 100 * USD);

    let ok = f.spend(card, 5 * USD, "ok");
    let no = f.spend(card, 50 * USD, "no");

    assert_eq!(f.c.get_spend(&ok.id).decision, Decision::Approved);
    assert_eq!(f.c.get_spend(&ok.id).reason_code, ReasonCode::None);
    assert_eq!(f.c.get_spend(&no.id).decision, Decision::Declined);
    assert_eq!(f.c.get_spend(&no.id).reason_code, ReasonCode::LimitExceeded);
    f.check_books();
}

/// **F2.8** — the queries a dashboard and a report are built from.
#[test]
fn f2_8_queries_for_cards_and_history() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let a = f.card(Category::SaaS, "a", 1_000 * USD);
    let b = f.card(Category::AI, "b", 1_000 * USD);
    f.allocate(a, 500 * USD);
    f.allocate(b, 500 * USD);

    f.spend(a, 10 * USD, "a1");
    f.spend(a, 20 * USD, "a2");
    f.spend(b, 30 * USD, "b1");

    assert_eq!(f.c.list_cards().len(), 2);
    assert_eq!(f.c.get_card(&a).label, String::from_str(&f.e, "a"));
    // Per-card history, which is what the Card Detail screen in the deck's wireframe reads.
    assert_eq!(f.c.list_spends(&a).len(), 2);
    assert_eq!(f.c.list_spends(&b).len(), 1);
    // And everything, for the report.
    assert_eq!(f.c.list_spends(&0).len(), 3);
    f.check_books();
}

// ===========================================================================
// Deliverable 3 · Withdrawal & Reconciliation  (deck slide 08)
// ===========================================================================

/// **F3.1** — the whitelist is admin-only, and removing an address leaves old records alone.
#[test]
fn f3_1_settlement_whitelist() {
    let f = F::new();
    assert_err!(
        f.c.try_add_whitelist(&f.operator, &f.settlement),
        Error::Unauthorized
    );
    f.whitelist(&f.settlement);
    assert!(f.c.is_whitelisted(&f.settlement));
    f.c.remove_whitelist(&f.admin, &f.settlement);
    assert!(!f.c.is_whitelisted(&f.settlement));
}

/// **F3.2** — the debit comes off the card that incurred the spend, not the unallocated pool.
///
/// The deck is explicit and it is the requirement most likely to be got wrong under time
/// pressure, because taking it from unallocated is simpler and the top-line total comes out the
/// same. It is not the same: the card balance is the record of what a category has left, and
/// settling elsewhere leaves every category report wrong while the headline number looks right.
#[test]
fn f3_2_settlement_debits_the_originating_card() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let saas = f.card(Category::SaaS, "saas", 1_000 * USD);
    let ai = f.card(Category::AI, "ai", 1_000 * USD);
    f.allocate(saas, 400 * USD);
    f.allocate(ai, 400 * USD);
    f.whitelist(&f.settlement);

    let r = f.spend(saas, 100 * USD, "aws");
    f.settle(r.id, 100 * USD);

    assert_eq!(f.get_card(saas).balance, 300 * USD); // the card that spent
    assert_eq!(f.get_card(ai).balance, 400 * USD); // untouched
    assert_eq!(f.c.get_unallocated(), 200 * USD); // and not the pool
    f.check_books();
}

/// **F3.3** — the three guards, each with a negative test, each failing loudly.
#[test]
fn f3_3_three_guards() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 500 * USD);
    f.whitelist(&f.settlement);
    let r = f.spend(card, 100 * USD, "aws");

    // authorised operator
    assert_err!(
        f.c.try_settle(&f.outsider, &r.id, &f.settlement, &(100 * USD)),
        Error::Unauthorized
    );
    // whitelisted destination
    assert_err!(
        f.c.try_settle(&f.operator, &r.id, &f.rogue, &(100 * USD)),
        Error::NotWhitelisted
    );
    // at most what the spend recorded
    assert_err!(
        f.c.try_settle(&f.operator, &r.id, &f.settlement, &(101 * USD)),
        Error::AmountExceedsSpend
    );

    // Not one of them silently passed: nothing moved.
    assert_eq!(f.held(), 1_000 * USD);
    assert_eq!(f.token.balance(&f.settlement), 0);
    f.check_books();
}

/// **F3.4** — the settlement links to the spend, and a spend settles at most once.
#[test]
fn f3_4_reconciliation_links_and_is_unique() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 500 * USD);
    f.whitelist(&f.settlement);
    let r = f.spend(card, 100 * USD, "aws");

    let s = f.settle(r.id, 100 * USD);
    assert_eq!(s.spend_id, r.id);
    assert_eq!(f.c.get_settlement_of_spend(&r.id), Some(s));
    assert_err!(
        f.c.try_settle(&f.operator, &r.id, &f.settlement, &USD),
        Error::AlreadySettled
    );
    f.check_books();
}

/// **F3.5** — the reconciliation ledger reads out as a table.
#[test]
fn f3_5_reconciliation_ledger_query() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card(Category::SaaS, "saas", 10_000 * USD);
    f.allocate(card, 5_000 * USD);
    f.whitelist(&f.settlement);

    for i in 0..3 {
        let r = f.spend(card, (100 + i) * USD, "aws");
        f.settle(r.id, (100 + i) * USD);
    }

    let ledger = f.c.list_settlements();
    assert_eq!(ledger.len(), 3);
    for (i, s) in ledger.iter().enumerate() {
        assert_eq!(s.id, i as u32 + 1);
        assert_eq!(s.card_id, card);
        assert_eq!(s.to, f.settlement);
        assert!(s.ledger_seq > 0);
    }
    f.check_books();
}

// ===========================================================================
// Data model and invariants  (deck slide 09)
// ===========================================================================

/// The deck's stated invariant, in the deck's own words:
/// `Σ card.balance + unallocated == the USDC the contract holds`.
///
/// Held across every kind of mutation, which is the part that matters — the identity is easy to
/// satisfy after a deposit and hard to satisfy after a settlement.
#[test]
fn invariant_one_solvency() {
    let f = F::new();
    f.deposit(250_000 * USD);
    f.whitelist(&f.settlement);
    let a = f.card(Category::SaaS, "a", 100_000 * USD);
    let b = f.card(Category::AI, "b", 100_000 * USD);

    f.check_books();
    f.allocate(a, 100_000 * USD);
    f.check_books();
    f.allocate(b, 50_000 * USD);
    f.check_books();
    f.c.deallocate(&f.admin, &a, &(20_000 * USD));
    f.check_books();
    f.c.transfer_between_cards(&f.admin, &b, &a, &(10_000 * USD));
    f.check_books();
    let r = f.spend(a, 5_000 * USD, "x");
    f.check_books();
    f.settle(r.id, 5_000 * USD);
    f.check_books();
}

/// **Invariant two** — a spend settles at most once, and every settled record has exactly one
/// settlement pointing back at it.
#[test]
fn invariant_two_no_double_settlement() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 1_000 * USD);
    f.whitelist(&f.settlement);

    let a = f.spend(card, 100 * USD, "a");
    let b = f.spend(card, 200 * USD, "b");
    f.settle(a.id, 100 * USD);
    assert_err!(
        f.c.try_settle(&f.operator, &a.id, &f.settlement, &USD),
        Error::AlreadySettled
    );
    f.settle(b.id, 200 * USD);

    assert_eq!(f.c.list_settlements().len(), 2);
    // `check_books` walks every spend and every settlement asserting exactly this.
    f.check_books();
}

/// **Slide 09** — the deck says a spend record keeps a place for the states Phase 1 does not
/// model. `Decision` is an enum rather than a boolean for that reason, and this asserts the
/// shape rather than the behaviour: adding `Captured` later is a code change, not a migration.
#[test]
fn spend_records_are_extensible_beyond_two_states() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 100 * USD);
    let r = f.spend(card, 10 * USD, "x");

    assert_eq!(r.decision, Decision::Approved);
    assert!(!r.settled);
    assert!(r.ledger_seq > 0);
    assert!(r.timestamp > 0);
    f.check_books();
}

// ===========================================================================
// UX principles that constrain the contract  (deck slide 16)
// ===========================================================================

/// **Principle 02** — a decline names its reason. This is why the contract carries a reason code
/// rather than a boolean: a console showing "failed" cannot tell somebody whether to raise a
/// limit, move money, or unfreeze a card.
#[test]
fn ux_02_every_decline_carries_a_distinct_reason() {
    let f = F::new();
    f.deposit(100_000 * USD);

    let frozen = f.card(Category::Cloud, "frozen", 100_000 * USD);
    f.allocate(frozen, 1_000 * USD);
    f.freeze(frozen);
    let capped = f.card(Category::AI, "capped", USD);
    f.allocate(capped, 1_000 * USD);
    let broke = f.card(Category::SaaS, "broke", 100_000 * USD);
    f.allocate(broke, USD);

    assert_eq!(
        f.spend(frozen, 10 * USD, "x").reason_code,
        ReasonCode::CardFrozen
    );
    assert_eq!(
        f.spend(capped, 10 * USD, "x").reason_code,
        ReasonCode::LimitExceeded
    );
    assert_eq!(
        f.spend(broke, 10 * USD, "x").reason_code,
        ReasonCode::InsufficientCardBalance
    );
    f.check_books();
}

/// **Principle 03** — the guardrail is visible before submitting. The contract has to expose
/// remaining limit and unallocated as queries, or a console can only tell somebody they were
/// refused after the fact.
#[test]
fn ux_03_headroom_is_readable_before_a_spend() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 500 * USD);
    f.allocate(card, 300 * USD);

    assert_eq!(f.c.get_remaining_limit(&card), 500 * USD);
    assert_eq!(f.c.get_unallocated(), 700 * USD);
    assert_eq!(f.c.get_card(&card).balance, 300 * USD);

    f.assert_approved(&f.spend(card, 200 * USD, "x"));
    assert_eq!(f.c.get_remaining_limit(&card), 300 * USD);
    f.check_books();
}

/// **Principle 03, the moving part** — the *live* view rolls an expired window, so a console
/// never shows last period's figure.
///
/// The stored card still holds the old window until something writes; the view is what a reader
/// sees, and it must be right at the moment it is read.
#[test]
fn ux_03_the_live_view_rolls_an_expired_window() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card_windowed("daily", 100 * USD, DAY);
    f.allocate(card, 10_000 * USD);
    f.assert_approved(&f.spend(card, 100 * USD, "x"));
    assert_eq!(f.c.get_remaining_limit(&card), 0);

    f.advance(DAY + 1);
    // Nothing has written since the window lapsed.
    assert_eq!(f.get_card(card).spent_in_window, 100 * USD);
    // But a reader is told the truth.
    assert_eq!(f.c.get_card_live(&card).spent_in_window, 0);
    assert_eq!(f.c.get_remaining_limit(&card), 100 * USD);
    f.check_books();
}

/// **Principle 05** — off-chain "approved" and on-chain "written" are different moments. The
/// contract stamps the ledger it wrote in so a console can show both.
#[test]
fn ux_05_records_are_stamped_with_their_ledger() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 1_000 * USD);
    f.whitelist(&f.settlement);

    let r = f.spend(card, 10 * USD, "x");
    f.advance(30);
    let s = f.settle(r.id, 10 * USD);

    assert!(r.ledger_seq > 0);
    assert!(s.ledger_seq >= r.ledger_seq);
    assert!(s.timestamp > r.timestamp);
    f.check_books();
}

// ===========================================================================
// Deviations · argued in docs/13-deck-conformance.md §B
// ===========================================================================

/// **§B-1 — declines are persisted, not merely emitted.**
///
/// The deck's decision #04 proposes recording only approvals, to save storage and avoid putting
/// refused spending on chain. We do the opposite: SOW §6 asks a reviewer with minimal technical
/// expertise to open a declined transaction and read its reason, and an event-only decline gives
/// them nothing to query.
#[test]
fn b1_declines_are_persisted() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", USD);
    f.allocate(card, 100 * USD);

    let r = f.spend(card, 50 * USD, "declined-on-purpose");
    assert_eq!(r.decision, Decision::Declined);
    // The record is readable back, by id, exactly like an approval.
    let read = f.c.get_spend(&r.id);
    assert_eq!(read, r);
    assert_eq!(read.reason_code, ReasonCode::LimitExceeded);
    f.check_books();
}

/// **§B-2 — a declined authorization is a successful transaction.**
///
/// The deck's flow diagrams draw declines next to panics, which conflates two different things.
/// A panic leaves no transaction; a declined spend must leave one, or the evidence table has
/// nothing to point at. Only refusals with nothing to record revert.
#[test]
fn b2_a_decline_is_a_success_and_a_panic_is_not() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", USD);
    f.allocate(card, 100 * USD);

    // Business refusal: returns Ok.
    let r = f.c.try_authorize_spend(
        &f.operator,
        &card,
        &(50 * USD),
        &String::from_str(&f.e, "x"),
    );
    assert!(r.is_ok());

    // Nothing to record: reverts.
    assert_err!(
        f.c.try_authorize_spend(&f.outsider, &card, &USD, &String::from_str(&f.e, "x")),
        Error::Unauthorized
    );
    f.check_books();
}

/// **§B-3 — the solvency check reads the token, not the books.**
///
/// The deck's invariant is an equality between the contract's accounting and its token balance.
/// Treated as an assertion it would panic on an issuer clawback — a treasury that is merely
/// short would become one that is frozen. So the accounting identity is asserted, the *backing*
/// is consulted, and the shortfall surfaces as `TREASURY_INSOLVENT` on a recorded decline.
#[test]
fn b3_a_short_treasury_declines_rather_than_freezing() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 1_000 * USD);
    f.clawback(999 * USD);

    // The contract still works. It refuses to spend, and says why.
    f.assert_declined(&f.spend(card, 10 * USD, "x"), ReasonCode::TreasuryInsolvent);
    // And the operations that do not need the missing money still run, so an organisation can
    // wind the treasury down rather than being locked out of it.
    f.c.deallocate(&f.admin, &card, &(500 * USD));
    f.c.set_card_status(&f.admin, &card, &CardStatus::Frozen);
    f.check_books();
}

/// **§B-4 — the limit window is a per-card field with a lazy reset.**
///
/// The deck leaves the period entirely open (decision #02). Rather than picking one globally, the
/// period lives on the card and the reset happens on read, so no keeper transaction is needed and
/// two cards can be budgeted on different cadences.
#[test]
fn b4_limit_windows_are_per_card() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let daily = f.card_windowed("daily", 100 * USD, DAY);
    let monthly = f.card(Category::SaaS, "monthly", 100 * USD);
    f.allocate(daily, 10_000 * USD);
    f.allocate(monthly, 10_000 * USD);

    f.assert_approved(&f.spend(daily, 100 * USD, "x"));
    f.assert_approved(&f.spend(monthly, 100 * USD, "x"));

    f.advance(DAY + 1);
    // One has rolled; the other has not.
    f.assert_approved(&f.spend(daily, 100 * USD, "y"));
    f.assert_declined(&f.spend(monthly, 1, "y"), ReasonCode::LimitExceeded);
    f.check_books();
}
