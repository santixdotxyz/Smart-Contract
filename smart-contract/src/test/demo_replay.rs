//! The acceptance scenario, run in process.
//!
//! # Why the same scenario exists twice
//!
//! `scripts/demo.sh` drives this against testnet and writes `.deploy/evidence.md`. That run
//! costs funded accounts, network round trips and several minutes, and when a step is wrong it
//! fails somewhere in the middle with a transaction already on chain.
//!
//! This is the same sequence, in the same order, with the same expected outcomes, and it runs in
//! under a second. Anything wrong with the scenario itself — a figure that no longer adds up, a
//! step that expects the wrong error, an ordering that stopped making sense after a change to
//! the contract — fails here first.
//!
//! **They must be kept in step.** If you change the scenario in one, change it in the other; the
//! step numbers below are the step numbers the script prints and the rows the evidence table
//! carries. `docs/10-testing.md` §Acceptance is the description both are written from.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{Category, ReasonCode};

/// The full end-to-end path the SOW asks to see: deposit → cards → allocate → authorize →
/// withdraw → reconcile, with every refusal on the way exercised and its exact code asserted.
#[test]
fn the_acceptance_scenario() {
    let f = F::new();

    // --- 1. Deposit -----------------------------------------------------------------
    f.deposit(250_000 * USD);
    assert_eq!(f.c.get_treasury().total_deposited, 250_000 * USD);
    assert_eq!(f.c.get_unallocated(), 250_000 * USD);
    assert_eq!(f.held(), 250_000 * USD);
    f.check_books();

    // --- 2. Four cards, one per category -------------------------------------------
    let saas = f.card(Category::SaaS, "saas", 50_000 * USD);
    let ai = f.card(Category::AI, "ai-tools", 50_000 * USD);
    let ads = f.card(Category::Advertising, "advertising", 50_000 * USD);
    let cloud = f.card(Category::Cloud, "cloud", 25_000 * USD);
    assert_eq!(f.c.list_cards().len(), 4);

    // --- 3. Allocate ----------------------------------------------------------------
    f.allocate(saas, 42_300 * USD);
    f.allocate(ai, 61_000 * USD);
    f.allocate(ads, 58_400 * USD);
    f.allocate(cloud, 25_000 * USD);
    assert_eq!(f.c.get_unallocated(), 63_300 * USD);
    f.check_books();

    // --- 4. Deallocate, then rebalance ----------------------------------------------
    // Both are in the scenario because they are the two ways money moves without leaving, and
    // the SOW asks for a transaction hash proving each.
    f.c.deallocate(&f.admin, &ads, &(4_200 * USD));
    assert_eq!(f.get_card(ads).balance, 54_200 * USD);
    assert_eq!(f.c.get_unallocated(), 67_500 * USD);

    f.c.transfer_between_cards(&f.admin, &ai, &saas, &(1_000 * USD));
    assert_eq!(f.get_card(ai).balance, 60_000 * USD);
    assert_eq!(f.get_card(saas).balance, 43_300 * USD);
    // A rebalance is invisible to the treasury, and must be.
    assert_eq!(f.c.get_unallocated(), 67_500 * USD);
    f.check_books();

    // --- 5. Limits and status --------------------------------------------------------
    f.c.set_limit(&f.admin, &ai, &(50_000 * USD), &0);
    f.freeze(cloud);
    assert_eq!(f.get_card(cloud).status, crate::types::CardStatus::Frozen);

    // --- 6. An approved spend --------------------------------------------------------
    let approved = f.spend(saas, 2_400 * USD, "aws-invoice-0824");
    f.assert_approved(&approved);
    // Authorization moves nothing. The card is debited at settlement.
    assert_eq!(f.get_card(saas).balance, 43_300 * USD);
    assert_eq!(f.held(), 250_000 * USD);
    f.check_books();

    // --- 7-9. Three declines, three different reasons --------------------------------
    // Each of these SUCCEEDS as a transaction and writes a record. That is decision #04, and it
    // is what gives the evidence table a declined row a reviewer can open.
    f.assert_declined(
        &f.spend(cloud, 500 * USD, "gcp-frozen-card"),
        ReasonCode::CardFrozen,
    );

    f.assert_approved(&f.spend(ai, 37_000 * USD, "openai-sub"));
    assert_eq!(f.c.get_remaining_limit(&ai), 13_000 * USD);
    f.assert_declined(
        &f.spend(ai, 18_900 * USD, "openai-sub-0824"),
        ReasonCode::LimitExceeded,
    );

    // Balance short, limit fine: a card with plenty of headroom and not enough money.
    let small = f.card(Category::Other, "misc", 90_000 * USD);
    f.allocate(small, 100 * USD);
    f.assert_declined(
        &f.spend(small, 5_000 * USD, "misc-oversized"),
        ReasonCode::InsufficientCardBalance,
    );
    f.check_books();

    // --- 10-11. Two refusals that revert ---------------------------------------------
    // Unlike the three above, these produce no transaction on chain: the CLI simulates before
    // submitting, so they never reach the ledger. The evidence table labels them `simulation`
    // rather than inventing a hash.
    assert_err!(
        f.c.try_allocate(&f.admin, &saas, &(500_000 * USD)),
        Error::OverAllocation
    );
    assert_err!(
        f.c.try_allocate(&f.outsider, &saas, &USD),
        Error::Unauthorized
    );
    f.check_books();

    // --- 12. Whitelist ----------------------------------------------------------------
    f.whitelist(&f.settlement);
    assert!(f.c.is_whitelisted(&f.settlement));
    assert!(!f.c.is_whitelisted(&f.rogue));

    // --- 13. Settle — the only step where money leaves ---------------------------------
    let s = f.settle(approved.id, 2_400 * USD);
    assert_eq!(s.spend_id, approved.id);
    assert_eq!(s.card_id, saas);
    assert_eq!(f.get_card(saas).balance, 40_900 * USD);
    assert_eq!(f.token.balance(&f.settlement), 2_400 * USD);
    assert_eq!(f.held(), 247_600 * USD);
    assert!(f.c.get_spend(&approved.id).settled);
    f.check_books();

    // --- 14-17. Four ways settlement is refused ----------------------------------------
    assert_err!(
        f.c.try_settle(&f.operator, &approved.id, &f.settlement, &(2_400 * USD)),
        Error::AlreadySettled
    );

    let second = f.spend(ads, 12_000 * USD, "google-ads-0824");
    f.assert_approved(&second);
    assert_err!(
        f.c.try_settle(&f.operator, &second.id, &f.settlement, &(12_001 * USD)),
        Error::AmountExceedsSpend
    );
    assert_err!(
        f.c.try_settle(&f.admin, &second.id, &f.settlement, &(12_000 * USD)),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_settle(&f.operator, &second.id, &f.rogue, &(12_000 * USD)),
        Error::NotWhitelisted
    );
    f.check_books();

    // --- 18. A second settlement, so the reconciliation ledger has more than one row ----
    f.settle(second.id, 12_000 * USD);

    // --- 19. The reconciliation ledger, which is the evidence for Deliverable 3 ---------
    let ledger = f.c.list_settlements();
    assert_eq!(ledger.len(), 2);
    assert_eq!(ledger.get(0).unwrap().spend_id, approved.id);
    assert_eq!(ledger.get(1).unwrap().spend_id, second.id);

    // Every spend in the ledger points at a card, and every settled spend has exactly one
    // settlement. That is the round trip a reviewer follows from a transaction hash back to the
    // category that incurred the cost.
    for st in ledger.iter() {
        let sp = f.c.get_spend(&st.spend_id);
        assert!(sp.settled);
        assert_eq!(sp.card_id, st.card_id);
        assert_eq!(f.c.get_settlement_of_spend(&st.spend_id), Some(st));
    }

    let t = f.c.get_treasury();
    assert_eq!(t.total_deposited, 250_000 * USD);
    assert_eq!(t.total_settled, 14_400 * USD);
    assert_eq!(f.held(), 235_600 * USD);
    assert_eq!(f.token.balance(&f.settlement), 14_400 * USD);

    // Six spend records: three approved, three declined. The count is asserted because a
    // scenario that quietly stopped recording declines would still pass every other assertion
    // here, and decision #04 would have been undone without anything failing.
    let spends = f.c.list_spends(&0);
    assert_eq!(spends.len(), 6);
    assert_eq!(
        spends
            .iter()
            .filter(|s| s.decision == crate::types::Decision::Declined)
            .count(),
        3
    );

    f.check_books();
}
