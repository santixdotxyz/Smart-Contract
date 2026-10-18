//! Modules M3 and M4 — money in, and the three numbers everything else is measured against.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::Category;

#[test]
fn deposit_moves_tokens_and_the_totals_together() {
    let f = F::new();
    let before = f.token.balance(&f.funder);

    f.deposit(120_000 * USD);

    let t = f.c.get_treasury();
    assert_eq!(t.total_deposited, 120_000 * USD);
    assert_eq!(t.total_allocated, 0);
    assert_eq!(f.c.get_unallocated(), 120_000 * USD);

    // The token actually moved. This is the assertion the contract's own invariant check cannot
    // make, and the one a `deposit` that updated its books and forgot the transfer would fail.
    assert_eq!(f.held(), 120_000 * USD);
    assert_eq!(f.token.balance(&f.funder), before - 120_000 * USD);
    f.check_books();
}

#[test]
fn deposits_accumulate() {
    let f = F::new();
    f.deposit(120_000 * USD);
    f.deposit(80_000 * USD);
    f.deposit(50_000 * USD);

    assert_eq!(f.c.get_treasury().total_deposited, 250_000 * USD);
    assert_eq!(f.held(), 250_000 * USD);
    f.check_books();
}

/// Zero as well as negative. A zero-value deposit is a transaction that says nothing and a row
/// the reconciliation table has to explain.
#[test]
fn a_non_positive_deposit_is_refused() {
    let f = F::new();
    assert_err!(f.c.try_deposit(&f.funder, &0), Error::InvalidAmount);
    assert_err!(f.c.try_deposit(&f.funder, &-1), Error::InvalidAmount);
    assert_eq!(f.held(), 0);
    f.check_books();
}

/// Anyone may fund the treasury. There is no allowlist of depositors, and there does not need to
/// be: money arriving is never the problem, and requiring a role would mean an organisation
/// could not accept a transfer from its own second wallet without a governance step.
#[test]
fn anyone_may_deposit() {
    let f = F::new();
    f.token.mint(&f.outsider, &(10_000 * USD));
    f.c.deposit(&f.outsider, &(10_000 * USD));

    assert_eq!(f.c.get_treasury().total_deposited, 10_000 * USD);
    f.check_books();
}

/// The deck's stated invariant, in its own words, across a full round trip.
#[test]
fn the_three_numbers_stay_consistent_through_a_whole_cycle() {
    let f = F::new();
    f.deposit(250_000 * USD);

    let saas = f.card(Category::SaaS, "saas", 50_000 * USD);
    let ai = f.card(Category::AI, "ai-tools", 50_000 * USD);
    f.allocate(saas, 42_300 * USD);
    f.allocate(ai, 61_000 * USD);
    f.check_books();

    assert_eq!(f.c.get_treasury().total_allocated, 103_300 * USD);
    assert_eq!(f.c.get_unallocated(), 146_700 * USD);

    f.c.deallocate(&f.admin, &saas, &(2_300 * USD));
    assert_eq!(f.c.get_unallocated(), 149_000 * USD);
    f.check_books();

    // A rebalance moves nothing in the treasury: it is a transfer inside the allocated pool.
    f.c.transfer_between_cards(&f.admin, &ai, &saas, &(1_000 * USD));
    assert_eq!(f.c.get_treasury().total_allocated, 101_000 * USD);
    assert_eq!(f.c.get_unallocated(), 149_000 * USD);
    assert_eq!(f.get_card(saas).balance, 41_000 * USD);
    assert_eq!(f.get_card(ai).balance, 60_000 * USD);
    f.check_books();

    // A settlement is the only thing that reduces what the contract holds.
    f.whitelist(&f.settlement);
    let spend = f.spend(saas, 2_400 * USD, "aws-invoice");
    f.assert_approved(&spend);
    f.settle(spend.id, 2_400 * USD);

    let t = f.c.get_treasury();
    assert_eq!(t.total_settled, 2_400 * USD);
    // total_allocated deliberately does NOT move on settle. That is what keeps
    // `Σ card.balance = total_allocated − total_settled` true.
    assert_eq!(t.total_allocated, 101_000 * USD);
    assert_eq!(f.c.get_unallocated(), 149_000 * USD);
    assert_eq!(f.held(), 247_600 * USD);
    f.check_books();
}
