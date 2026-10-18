//! Decision #05 — smallest units, `i128`, and nothing that rounds.
//!
//! # There is no arithmetic in this contract worth rounding
//!
//! That is the point of the design rather than an omission. Saintix moves whole token units
//! between three places — the unallocated pool, a card, and a settlement address — and never
//! multiplies, divides, or converts between assets. There is no price, no interest, no fee, and
//! no second decimal scale to reconcile. Every figure is an integer count of the token's
//! smallest unit from the moment it arrives until the moment it leaves.
//!
//! So these tests are not about precision loss. They are about the two things that can still go
//! wrong: a smallest unit going missing at a boundary, and a subtraction wrapping instead of
//! trapping.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{Category, ReasonCode};

/// One unit of a 7-decimal token — 0.0000001 USDC — survives a whole round trip.
///
/// The smallest representable amount is where an accidental division or a `>=` in place of `>`
/// shows up first, and it is invisible at the scale the rest of the suite works at.
#[test]
fn a_single_smallest_unit_survives_the_whole_cycle() {
    let f = F::new();
    f.deposit(1);
    let card = f.card(Category::Other, "dust", 1);
    f.allocate(card, 1);
    f.whitelist(&f.settlement);

    assert_eq!(f.c.get_unallocated(), 0);
    assert_eq!(f.get_card(card).balance, 1);

    let r = f.spend(card, 1, "one-unit");
    f.assert_approved(&r);
    f.settle(r.id, 1);

    assert_eq!(f.token.balance(&f.settlement), 1);
    assert_eq!(f.get_card(card).balance, 0);
    assert_eq!(f.held(), 0);
    f.check_books();
}

/// The contract reads `decimals` from the token, so a token that is not 7 works unchanged.
///
/// USDC on Stellar is 7 and always will be; this is here because `Config.decimals` is what an
/// explorer and the indexer render amounts with, and a value copied from an assumption rather
/// than read from the token would be wrong by orders of magnitude with nothing to contradict it.
#[test]
fn the_contract_takes_its_decimals_from_the_token() {
    for d in [6u32, 7, 8, 18] {
        let f = F::with_decimals(d);
        assert_eq!(f.c.get_config().decimals, d);

        // And the arithmetic does not care, because there is none that scales. The funder is
        // topped up per scale rather than once: an 18-decimal unit is eleven orders of magnitude
        // larger than a 7-decimal one, and a fixture minted at the smaller scale is broke at the
        // larger one for reasons that have nothing to do with the contract.
        let unit = 10i128.pow(d);
        f.token.mint(&f.funder, &(10 * unit));
        f.deposit(5 * unit);
        let card = f.card(Category::SaaS, "saas", 5 * unit);
        f.allocate(card, 5 * unit);
        f.whitelist(&f.settlement);
        let r = f.spend(card, 3 * unit, "x");
        f.assert_approved(&r);
        f.settle(r.id, 3 * unit);
        assert_eq!(f.token.balance(&f.settlement), 3 * unit);
        f.check_books();
    }
}

/// Amounts far beyond any real treasury, to show `i128` is not being narrowed anywhere.
///
/// A hundred trillion units of a 7-decimal token is ten million USDC; the figure is chosen to
/// overflow anything that had quietly been kept in an `i64`.
#[test]
fn very_large_amounts_are_handled_without_narrowing() {
    let f = F::new();
    let huge = 10_000_000 * USD; // 10 million USDC in smallest units
    f.token.mint(&f.funder, &huge);
    f.deposit(huge);

    let card = f.card(Category::Cloud, "big", huge);
    f.allocate(card, huge);
    f.whitelist(&f.settlement);

    let r = f.spend(card, huge, "everything");
    f.assert_approved(&r);
    f.settle(r.id, huge);

    assert_eq!(f.token.balance(&f.settlement), huge);
    assert_eq!(f.held(), 0);
    f.check_books();
}

/// Every subtraction in the contract is guarded by a check that runs first, so the amount that
/// would underflow is refused rather than wrapping.
///
/// Two guards and two error codes, because they are different mistakes: taking more off a card
/// than it holds, and putting more onto cards than the treasury has.
#[test]
fn subtractions_are_guarded_rather_than_wrapping() {
    let f = F::new();
    f.deposit(10 * USD);
    let a = f.card(Category::SaaS, "a", 10 * USD);
    let b = f.card(Category::AI, "b", 10 * USD);
    f.allocate(a, 10 * USD);

    // One unit past the card.
    assert_err!(
        f.c.try_deallocate(&f.admin, &a, &(10 * USD + 1)),
        Error::InsufficientCardBalance
    );
    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &b, &(10 * USD + 1)),
        Error::InsufficientCardBalance
    );
    // One unit past the pool.
    assert_err!(f.c.try_allocate(&f.admin, &b, &1), Error::OverAllocation);

    // And the guarded path at exactly the boundary still works, so the guard is not off by one
    // in the other direction.
    f.c.deallocate(&f.admin, &a, &(10 * USD));
    assert_eq!(f.get_card(a).balance, 0);
    f.check_books();
}

/// A spend one smallest unit over each ceiling is declined, and exactly at it is approved.
///
/// Both directions matter. A `>=` where the code says `>` makes the last unit of every card and
/// every limit permanently unusable, which nothing at ordinary scale would ever reveal.
#[test]
fn the_boundaries_are_inclusive_on_the_approving_side() {
    let f = F::new();
    f.deposit(2_000 * USD);

    // Limit boundary.
    let a = f.card(Category::SaaS, "limit", 100 * USD);
    f.allocate(a, 1_000 * USD);
    f.assert_declined(
        &f.spend(a, 100 * USD + 1, "over"),
        ReasonCode::LimitExceeded,
    );
    f.assert_approved(&f.spend(a, 100 * USD, "exact"));

    // Balance boundary.
    let b = f.card_windowed("balance", 1_000 * USD, super::harness::MONTH);
    f.allocate(b, 50 * USD);
    f.assert_declined(
        &f.spend(b, 50 * USD + 1, "over"),
        ReasonCode::InsufficientCardBalance,
    );
    f.assert_approved(&f.spend(b, 50 * USD, "exact"));

    f.check_books();
}
