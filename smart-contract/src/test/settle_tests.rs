//! Modules M10, M11 and M12 — the whitelist, the withdrawal, and the reconciliation record.
//!
//! This is the only place money leaves the contract, so it is the only place where a guard
//! failing open costs anything.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::Category;

fn ready() -> (F<'static>, u32, u32) {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 90_000 * USD);
    f.allocate(card, 50_000 * USD);
    f.whitelist(&f.settlement);
    let spend = f.spend(card, 2_400 * USD, "aws-invoice-0824").id;
    (f, card, spend)
}

// ---------------------------------------------------------------------------
// M10 · whitelist
// ---------------------------------------------------------------------------

#[test]
fn whitelist_add_and_remove() {
    let f = F::new();
    assert!(!f.c.is_whitelisted(&f.settlement));

    f.whitelist(&f.settlement);
    assert!(f.c.is_whitelisted(&f.settlement));
    assert_eq!(f.c.list_whitelist().len(), 1);

    f.c.remove_whitelist(&f.admin, &f.settlement);
    assert!(!f.c.is_whitelisted(&f.settlement));
}

/// A no-op is refused rather than silently succeeding. A script that reports "whitelisted" for a
/// call that changed nothing is a script whose output cannot be trusted as evidence.
#[test]
fn a_whitelist_change_that_changes_nothing_is_refused() {
    let f = F::new();
    f.whitelist(&f.settlement);
    assert_err!(
        f.c.try_add_whitelist(&f.admin, &f.settlement),
        Error::WhitelistUnchanged
    );
    assert_err!(
        f.c.try_remove_whitelist(&f.admin, &f.rogue),
        Error::WhitelistUnchanged
    );
}

/// T9. The second of the three guards the deck names.
#[test]
fn settling_to_an_address_that_is_not_whitelisted_is_refused() {
    let (f, _card, spend) = ready();
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.rogue, &(2_400 * USD)),
        Error::NotWhitelisted
    );
    assert_eq!(f.held(), 100_000 * USD);
    f.check_books();
}

/// Removing an address does not rewrite the settlements already made to it. A reconciliation
/// record is a statement about what happened; editing history to match current policy would make
/// the ledger useless as evidence.
#[test]
fn removing_an_address_leaves_past_settlements_alone() {
    let (f, card, spend) = ready();
    let s = f.settle(spend, 2_400 * USD);
    f.c.remove_whitelist(&f.admin, &f.settlement);

    let recorded = f.c.get_settlement(&s.id);
    assert_eq!(recorded.to, f.settlement);
    assert_eq!(recorded.amount, 2_400 * USD);
    assert_eq!(recorded.card_id, card);

    // But nothing new may go there.
    let next = f.spend(card, 100 * USD, "again").id;
    assert_err!(
        f.c.try_settle(&f.operator, &next, &f.settlement, &(100 * USD)),
        Error::NotWhitelisted
    );
    f.check_books();
}

// ---------------------------------------------------------------------------
// M11 · withdrawal
// ---------------------------------------------------------------------------

/// The happy path, and the one a reviewer opens first.
#[test]
fn settling_debits_the_originating_card_and_moves_the_tokens() {
    let (f, card, spend) = ready();

    let s = f.settle(spend, 2_400 * USD);

    // The card that incurred the spend is the card that paid — not the unallocated pool. Taking
    // it from unallocated would leave the top-line number right and every category report wrong.
    assert_eq!(f.get_card(card).balance, 47_600 * USD);
    assert_eq!(f.c.get_unallocated(), 50_000 * USD);

    assert_eq!(f.c.get_treasury().total_settled, 2_400 * USD);
    assert_eq!(f.held(), 97_600 * USD);
    assert_eq!(f.token.balance(&f.settlement), 2_400 * USD);

    // The link that makes a transaction hash into evidence.
    assert_eq!(s.spend_id, spend);
    assert_eq!(s.card_id, card);
    assert!(f.c.get_spend(&spend).settled);
    assert_eq!(f.c.get_settlement_of_spend(&spend), Some(s));
    f.check_books();
}

/// T8. The first of the three guards.
#[test]
fn only_an_operator_may_settle() {
    let (f, _card, spend) = ready();
    assert_err!(
        f.c.try_settle(&f.admin, &spend, &f.settlement, &(100 * USD)),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_settle(&f.outsider, &spend, &f.settlement, &(100 * USD)),
        Error::Unauthorized
    );
    assert_eq!(f.held(), 100_000 * USD);
    f.check_books();
}

/// T10, first half. The third guard.
#[test]
fn settling_more_than_the_spend_is_refused() {
    let (f, _card, spend) = ready();
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &(2_401 * USD)),
        Error::AmountExceedsSpend
    );
    f.check_books();
}

/// Settling for **less** is allowed, and deliberately so: a real settlement often is less — a
/// partial capture, a discount applied after authorization. Refusing it would force an operator
/// to choose between over-paying and leaving the spend open forever.
#[test]
fn settling_for_less_than_the_spend_is_allowed() {
    let (f, card, spend) = ready();
    f.settle(spend, 2_000 * USD);

    assert_eq!(f.get_card(card).balance, 48_000 * USD);
    assert_eq!(f.token.balance(&f.settlement), 2_000 * USD);
    // And the spend is closed at the lower figure — there is no partial state to chase.
    assert!(f.c.get_spend(&spend).settled);
    f.check_books();
}

/// A declined spend has a record precisely because the contract refused it. Settling one would
/// make the refusal decorative.
#[test]
fn a_declined_spend_cannot_be_settled() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 50_000 * USD);
    f.whitelist(&f.settlement);

    let declined = f.spend(card, 5_000 * USD, "over-limit").id;
    assert_err!(
        f.c.try_settle(&f.operator, &declined, &f.settlement, &(100 * USD)),
        Error::SpendNotApproved
    );
    assert_eq!(f.held(), 100_000 * USD);
    f.check_books();
}

#[test]
fn settling_a_spend_that_does_not_exist_is_refused() {
    let (f, _card, _spend) = ready();
    assert_err!(
        f.c.try_settle(&f.operator, &999, &f.settlement, &USD),
        Error::SpendNotFound
    );
    f.check_books();
}

#[test]
fn a_non_positive_settlement_is_refused() {
    let (f, _card, spend) = ready();
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &0),
        Error::InvalidAmount
    );
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &-1),
        Error::InvalidAmount
    );
    f.check_books();
}

// ---------------------------------------------------------------------------
// M12 · reconciliation
// ---------------------------------------------------------------------------

/// T10, second half. Invariant I3.
#[test]
fn a_spend_cannot_be_settled_twice() {
    let (f, _card, spend) = ready();
    f.settle(spend, 1_000 * USD);

    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &(1_000 * USD)),
        Error::AlreadySettled
    );
    // Even for the remainder. A spend closes once, at whatever figure it closed for.
    assert_err!(
        f.c.try_settle(&f.operator, &spend, &f.settlement, &(1_400 * USD)),
        Error::AlreadySettled
    );

    assert_eq!(f.c.list_settlements().len(), 1);
    assert_eq!(f.token.balance(&f.settlement), 1_000 * USD);
    f.check_books();
}

/// The reconciliation ledger as a whole — what the indexer's `report` renders and what the
/// evidence table is built from.
#[test]
fn the_reconciliation_ledger_links_every_settlement_to_its_spend() {
    let f = F::new();
    f.deposit(250_000 * USD);
    f.whitelist(&f.settlement);

    let saas = f.card(Category::SaaS, "saas", 50_000 * USD);
    let ads = f.card(Category::Advertising, "ads", 50_000 * USD);
    f.allocate(saas, 40_000 * USD);
    f.allocate(ads, 40_000 * USD);

    let a = f.spend(saas, 2_400 * USD, "aws").id;
    let b = f.spend(ads, 12_000 * USD, "google-ads").id;
    let c = f.spend(saas, 960 * USD, "figma").id;

    f.settle(a, 2_400 * USD);
    f.settle(b, 12_000 * USD);
    // `c` is left open on purpose: the deck's Reconciliation wireframe shows exactly this — two
    // of three settled, one still pending.

    let ledger = f.c.list_settlements();
    assert_eq!(ledger.len(), 2);
    assert_eq!(ledger.get(0).unwrap().spend_id, a);
    assert_eq!(ledger.get(0).unwrap().card_id, saas);
    assert_eq!(ledger.get(1).unwrap().spend_id, b);
    assert_eq!(ledger.get(1).unwrap().card_id, ads);

    assert!(f.c.get_settlement_of_spend(&c).is_none());
    assert!(!f.c.get_spend(&c).settled);

    assert_eq!(f.get_card(saas).balance, 37_600 * USD);
    assert_eq!(f.get_card(ads).balance, 28_000 * USD);
    assert_eq!(f.c.get_treasury().total_settled, 14_400 * USD);
    assert_eq!(f.held(), 235_600 * USD);
    f.check_books();
}
