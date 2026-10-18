//! The ten cases the deck names (slide 22), one test each, in order.
//!
//! # Why this file duplicates coverage that already exists
//!
//! Every case here is also covered somewhere in `spend_tests.rs`, `cards_tests.rs` or
//! `settle_tests.rs`, usually alongside its neighbours. This file exists anyway, because the
//! deck's test matrix is what a Chapter Lead checks against and a reviewer should not have to
//! work out which of sixty tests corresponds to T4. The names are the case numbers, the module
//! is ordered like the table, and `13-deck-conformance.md` cites these by name.
//!
//! # The extra layer the deck asks for
//!
//! > *"After **each** case above, assert the solvency invariant again. Cheap to write, and it is
//! > the thing that catches an accounting error that no individual case sees."*
//!
//! Every test here ends in `check_books()`, which does that and more: it sums the actual cards
//! rather than trusting the running totals, and compares against the token's real balance.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{Category, ReasonCode};

/// **T1** — Active card, under limit, sufficient balance → `APPROVED`.
#[test]
fn t1_approved_spend() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 50_000 * USD);
    f.allocate(card, 42_300 * USD);

    let r = f.spend(card, 2_400 * USD, "aws-invoice");
    f.assert_approved(&r);
    assert_eq!(f.get_card(card).spent_in_window, 2_400 * USD);
    f.check_books();
}

/// **T2** — Card frozen → `CARD_FROZEN`.
#[test]
fn t2_frozen_card() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::Cloud, "cloud", 25_000 * USD);
    f.allocate(card, 25_000 * USD);
    f.freeze(card);

    f.assert_declined(&f.spend(card, 100 * USD, "gcp"), ReasonCode::CardFrozen);
    f.check_books();
}

/// **T3** — Amount over the remaining window limit → `LIMIT_EXCEEDED`.
///
/// The figures are the deck's own, from the Spend Simulator wireframe: a card with 37,000 of a
/// 50,000 limit already spent, asked for 18,900.
#[test]
fn t3_over_limit() {
    let f = F::new();
    f.deposit(200_000 * USD);
    let card = f.card(Category::AI, "ai-tools", 50_000 * USD);
    f.allocate(card, 61_000 * USD);
    f.assert_approved(&f.spend(card, 37_000 * USD, "openai"));

    assert_eq!(f.c.get_remaining_limit(&card), 13_000 * USD);
    f.assert_declined(
        &f.spend(card, 18_900 * USD, "openai-sub-0824"),
        ReasonCode::LimitExceeded,
    );
    f.check_books();
}

/// **T4** — Limit available, card balance short → `INSUFFICIENT_CARD_BALANCE`.
#[test]
fn t4_insufficient_card_balance() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::Advertising, "ads", 90_000 * USD);
    f.allocate(card, 5_000 * USD);

    assert!(f.c.get_remaining_limit(&card) > 6_000 * USD);
    f.assert_declined(
        &f.spend(card, 6_000 * USD, "google-ads"),
        ReasonCode::InsufficientCardBalance,
    );
    f.check_books();
}

/// **T5** — Card is fine, the treasury cannot pay → `TREASURY_INSOLVENT`.
///
/// Reached the only honest way: an issuer clawback removes tokens the contract's own books still
/// count. Check 4 reads the token rather than the books, which is what makes this detectable.
#[test]
fn t5_treasury_insolvent() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 90_000 * USD);
    f.allocate(card, 80_000 * USD);

    f.clawback(99_900 * USD);

    f.assert_declined(
        &f.spend(card, 1_000 * USD, "aws"),
        ReasonCode::TreasuryInsolvent,
    );
    f.check_books();
}

/// **T6** — Allocate past the unallocated balance → `OVER_ALLOCATION`.
#[test]
fn t6_over_allocation() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 0);

    assert_err!(
        f.c.try_allocate(&f.admin, &card, &(100_001 * USD)),
        Error::OverAllocation
    );
    f.check_books();
}

/// **T7** — A non-admin calls `allocate` → `UNAUTHORIZED`.
#[test]
fn t7_allocate_requires_admin() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 0);

    assert_err!(
        f.c.try_allocate(&f.outsider, &card, &(10 * USD)),
        Error::Unauthorized
    );
    // And an operator is not a lesser admin.
    assert_err!(
        f.c.try_allocate(&f.operator, &card, &(10 * USD)),
        Error::Unauthorized
    );
    f.check_books();
}

/// **T8** — A non-operator calls `settle` → `UNAUTHORIZED`.
#[test]
fn t8_settle_requires_operator() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 50_000 * USD);
    f.allocate(card, 50_000 * USD);
    f.whitelist(&f.settlement);
    let spend = f.spend(card, 1_000 * USD, "aws").id;

    assert_err!(
        f.c.try_settle(&f.admin, &spend, &f.settlement, &(1_000 * USD)),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_settle(&f.outsider, &spend, &f.settlement, &(1_000 * USD)),
        Error::Unauthorized
    );
    assert_eq!(f.held(), 100_000 * USD);
    f.check_books();
}

/// **T9** — Settle to an address that is not whitelisted → `NOT_WHITELISTED`.
#[test]
fn t9_settlement_address_must_be_whitelisted() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 50_000 * USD);
    f.allocate(card, 50_000 * USD);
    f.whitelist(&f.settlement);
    let spend = f.spend(card, 1_000 * USD, "aws").id;

    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.rogue, &(1_000 * USD)),
        Error::NotWhitelisted
    );
    assert_eq!(f.token.balance(&f.rogue), 0);
    f.check_books();
}

/// **T10** — Settle for more than the spend, and settle a second time →
/// `AMOUNT_EXCEEDS_SPEND`, `ALREADY_SETTLED`.
#[test]
fn t10_settlement_amount_and_double_settle() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 50_000 * USD);
    f.allocate(card, 50_000 * USD);
    f.whitelist(&f.settlement);
    let spend = f.spend(card, 1_000 * USD, "aws").id;

    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &(1_001 * USD)),
        Error::AmountExceedsSpend
    );

    f.settle(spend, 1_000 * USD);
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &(1_000 * USD)),
        Error::AlreadySettled
    );

    assert_eq!(f.c.list_settlements().len(), 1);
    assert_eq!(f.token.balance(&f.settlement), 1_000 * USD);
    f.check_books();
}

/// The four groups the SOW names, driven end to end in one scenario: an approved spend, a
/// declined spend, an over-allocation, and a role violation — with the books checked after each.
///
/// This is the shape `demo.sh` reproduces on testnet. Having it here means a broken scenario
/// fails in one second locally rather than after ten minutes of funded transactions.
#[test]
fn the_four_sow_case_groups_in_one_pass() {
    let f = F::new();
    f.deposit(250_000 * USD);
    f.whitelist(&f.settlement);

    let saas = f.card(Category::SaaS, "saas", 50_000 * USD);
    let cloud = f.card(Category::Cloud, "cloud", 25_000 * USD);
    f.allocate(saas, 42_300 * USD);
    f.allocate(cloud, 25_000 * USD);
    f.check_books();

    // approved spend
    let ok = f.spend(saas, 2_400 * USD, "aws");
    f.assert_approved(&ok);
    f.check_books();

    // declined spend
    f.freeze(cloud);
    f.assert_declined(&f.spend(cloud, 100 * USD, "gcp"), ReasonCode::CardFrozen);
    f.check_books();

    // over-limit
    f.assert_declined(
        &f.spend(saas, 60_000 * USD, "oracle"),
        ReasonCode::LimitExceeded,
    );
    f.check_books();

    // over-allocation
    assert_err!(
        f.c.try_allocate(&f.admin, &saas, &(200_000 * USD)),
        Error::OverAllocation
    );
    f.check_books();

    // role permission
    assert_err!(
        f.c.try_allocate(&f.outsider, &saas, &USD),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_settle(&f.admin, &ok.id, &f.settlement, &USD),
        Error::Unauthorized
    );
    f.check_books();

    // and the money out the far end
    f.settle(ok.id, 2_400 * USD);
    assert_eq!(f.token.balance(&f.settlement), 2_400 * USD);
    assert_eq!(f.get_card(saas).balance, 39_900 * USD);
    f.check_books();
}
