//! Module M9 — the four checks, the frozen order, and the fact that a decline is a success.

use soroban_sdk::String;

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{Category, Decision, ReasonCode};

/// T1. Everything in order.
#[test]
fn an_ordinary_spend_is_approved_and_recorded() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 50_000 * USD);
    f.allocate(card, 42_300 * USD);

    let r = f.spend(card, 2_400 * USD, "aws-invoice-0824");
    f.assert_approved(&r);
    assert_eq!(r.id, 1);
    assert_eq!(r.card_id, card);
    assert_eq!(r.amount, 2_400 * USD);
    assert_eq!(r.merchant_ref, String::from_str(&f.e, "aws-invoice-0824"));

    // The record is readable back, which is what the reconciliation view depends on.
    assert_eq!(f.c.get_spend(&r.id), r);
    assert_eq!(f.c.list_spends(&card).len(), 1);

    // Authorization moves no money. The card is debited later, by `settle`.
    assert_eq!(f.get_card(card).balance, 42_300 * USD);
    assert_eq!(f.held(), 100_000 * USD);
    f.check_books();
}

/// T2 — check 1.
#[test]
fn a_frozen_card_declines_before_anything_else_is_consulted() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::Cloud, "cloud", 25_000 * USD);
    f.allocate(card, 25_000 * USD);
    f.freeze(card);

    let r = f.spend(card, 100 * USD, "gcp");
    f.assert_declined(&r, ReasonCode::CardFrozen);
    f.check_books();
}

/// T4 — check 3. Limit is fine; the card is broke.
#[test]
fn a_card_short_of_balance_declines_on_balance() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::Advertising, "ads", 90_000 * USD);
    f.allocate(card, 5_000 * USD);

    let r = f.spend(card, 6_000 * USD, "google-ads");
    f.assert_declined(&r, ReasonCode::InsufficientCardBalance);
    f.check_books();
}

/// T5 — check 4, and the one state the contract's own books cannot produce.
///
/// The card has balance and limit to spare, and the treasury still cannot pay, because tokens
/// left the contract without its knowledge. On Stellar that is an issuer clawback. The check
/// reads the token's real balance rather than the running totals — which is the whole reason it
/// can catch this at all.
#[test]
fn a_clawed_back_treasury_declines_on_solvency() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 90_000 * USD);
    f.allocate(card, 80_000 * USD);

    f.assert_approved(&f.spend(card, 1_000 * USD, "before"));

    f.clawback(99_500 * USD);

    let r = f.spend(card, 1_000 * USD, "after");
    f.assert_declined(&r, ReasonCode::TreasuryInsolvent);
    f.check_books();
}

/// The frozen order, demonstrated where it actually matters: two conditions fail at once and the
/// answer is decided entirely by which check runs first.
///
/// If this order ever changes, the off-chain gateway and the contract start giving different
/// reasons for the same declined transaction, and the reconciliation table stops explaining
/// anything.
#[test]
fn the_check_order_decides_the_reason_when_several_fail() {
    let f = F::new();
    f.deposit(100_000 * USD);

    // Frozen AND over limit AND short of balance. Status wins.
    let a = f.card(Category::Cloud, "all-three", 10 * USD);
    f.allocate(a, USD);
    f.freeze(a);
    f.assert_declined(&f.spend(a, 5_000 * USD, "x"), ReasonCode::CardFrozen);

    // Over limit AND short of balance, not frozen. Limit wins.
    let b = f.card(Category::Cloud, "two", 10 * USD);
    f.allocate(b, USD);
    f.assert_declined(&f.spend(b, 5_000 * USD, "x"), ReasonCode::LimitExceeded);

    // Short of balance only.
    let c = f.card(Category::Cloud, "one", 50_000 * USD);
    f.allocate(c, USD);
    f.assert_declined(
        &f.spend(c, 5_000 * USD, "x"),
        ReasonCode::InsufficientCardBalance,
    );
    f.check_books();
}

/// Decision #04 in one assertion: a declined spend is a first-class row a reviewer can open, not
/// a reverted transaction that left nothing behind.
#[test]
fn declines_are_recorded_alongside_approvals() {
    let f = F::new();
    f.deposit(100_000 * USD);
    // A generous limit, so the second spend fails on balance rather than on the limit — the
    // point of this test is that three different outcomes all leave a row behind, and using the
    // same reason code twice would prove less.
    let card = f.card(Category::AI, "ai", 100_000 * USD);
    f.allocate(card, 100 * USD);

    f.spend(card, 50 * USD, "ok");
    f.spend(card, 5_000 * USD, "too-big");
    f.freeze(card);
    f.spend(card, USD, "frozen");

    let all = f.c.list_spends(&card);
    assert_eq!(all.len(), 3);
    assert_eq!(all.get(0).unwrap().decision, Decision::Approved);
    assert_eq!(
        all.get(1).unwrap().reason_code,
        ReasonCode::InsufficientCardBalance
    );
    assert_eq!(all.get(2).unwrap().reason_code, ReasonCode::CardFrozen);

    // Ids are handed out for declines too, so the ledger has no gaps to explain.
    assert_eq!(all.get(2).unwrap().id, 3);
    f.check_books();
}

/// The three cases where there is nothing sensible to record, and the call is refused instead.
#[test]
fn authorization_is_refused_when_there_is_nothing_to_record() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 100 * USD);

    // Not an operator.
    assert_err!(
        f.c.try_authorize_spend(&f.outsider, &card, &USD, &String::from_str(&f.e, "r")),
        Error::Unauthorized
    );
    // No such card.
    assert_err!(
        f.c.try_authorize_spend(&f.operator, &99, &USD, &String::from_str(&f.e, "r")),
        Error::CardNotFound
    );
    // Not an amount.
    assert_err!(
        f.c.try_authorize_spend(&f.operator, &card, &0, &String::from_str(&f.e, "r")),
        Error::InvalidAmount
    );
    // No merchant reference. A spend nobody can attribute is not evidence of anything.
    assert_err!(
        f.c.try_authorize_spend(&f.operator, &card, &USD, &String::from_str(&f.e, "")),
        Error::InvalidMerchantRef
    );

    // None of the four wrote a record.
    assert_eq!(f.c.list_spends(&0).len(), 0);
    f.check_books();
}

/// Records carry the ledger they were written in.
///
/// UX principle 05 in the deck: "approved" and "written on chain" are different moments, at least
/// one ledger apart, and a console that collapses them is lying about the race window.
#[test]
fn records_carry_the_ledger_they_were_written_in() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 100 * USD);

    let first = f.spend(card, USD, "a");
    f.advance(60);
    let second = f.spend(card, USD, "b");

    assert!(second.ledger_seq > first.ledger_seq);
    assert!(second.timestamp > first.timestamp);
    f.check_books();
}

/// The race window decision #01 accepts, made explicit.
///
/// Two authorizations against one card both pass check 3, because neither has settled and
/// nothing is held. The contract is the final arbiter: the first settlement succeeds and the
/// second is refused for want of balance. The gateway owns the orphan approval.
#[test]
fn two_approvals_can_exceed_the_card_and_settlement_is_the_arbiter() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 60 * USD);
    f.whitelist(&f.settlement);

    let a = f.spend(card, 40 * USD, "first");
    let b = f.spend(card, 40 * USD, "second");
    f.assert_approved(&a);
    f.assert_approved(&b);

    f.settle(a.id, 40 * USD);
    assert_err!(
        f.c.try_settle(&f.operator, &b.id, &f.settlement, &(40 * USD)),
        Error::InsufficientCardBalance
    );

    // The orphan is still an approved, unsettled record — visible, and the gateway's problem.
    let orphan = f.c.get_spend(&b.id);
    assert_eq!(orphan.decision, Decision::Approved);
    assert!(!orphan.settled);
    f.check_books();
}
