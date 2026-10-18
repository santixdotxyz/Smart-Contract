//! Modules M5, M6 and M8 — the registry, allocation, and status.

use soroban_sdk::String;

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{CardStatus, Category};

#[test]
fn card_ids_are_monotonic_and_start_at_one() {
    let f = F::new();
    assert_eq!(f.card(Category::SaaS, "saas", 0), 1);
    assert_eq!(f.card(Category::AI, "ai", 0), 2);
    assert_eq!(f.card(Category::Advertising, "ads", 0), 3);

    // Ids starting at 1 is what lets 0 mean "all cards" in `list_spends` without an Option in
    // the contract's public interface — which the CLI cannot express conveniently.
    assert_eq!(f.c.list_cards().len(), 3);
}

#[test]
fn a_new_card_starts_empty_active_and_in_a_fresh_window() {
    let f = F::new();
    let id = f.card(Category::Cloud, "cloud", 25_000 * USD);
    let c = f.get_card(id);

    assert_eq!(c.balance, 0);
    assert_eq!(c.limit, 25_000 * USD);
    assert_eq!(c.spent_in_window, 0);
    assert_eq!(c.status, CardStatus::Active);
    assert_eq!(c.limit_window_secs, crate::cards::DEFAULT_WINDOW_SECS);
    assert_eq!(c.category, Category::Cloud);
    f.check_books();
}

/// The five categories the deck names, each usable and each preserved.
#[test]
fn every_category_round_trips() {
    let f = F::new();
    for (i, cat) in [
        Category::SaaS,
        Category::AI,
        Category::Advertising,
        Category::Cloud,
        Category::Other,
    ]
    .into_iter()
    .enumerate()
    {
        let id = f.card(cat, "c", 0);
        assert_eq!(f.get_card(id).category, cat);
        assert_eq!(id, i as u32 + 1);
    }
}

#[test]
fn a_label_must_be_present_and_bounded() {
    let f = F::new();
    assert_err!(
        f.c.try_create_card(
            &f.admin,
            &Category::SaaS,
            &String::from_str(&f.e, ""),
            &0,
            &0
        ),
        Error::InvalidLabel
    );
    let long = String::from_str(&f.e, "0123456789012345678901234567890123456789");
    assert_err!(
        f.c.try_create_card(&f.admin, &Category::SaaS, &long, &0, &0),
        Error::InvalidLabel
    );
    // Exactly 32 is fine — the bound is inclusive.
    f.c.create_card(
        &f.admin,
        &Category::SaaS,
        &String::from_str(&f.e, "01234567890123456789012345678901"),
        &0,
        &0,
    );
}

#[test]
fn a_negative_limit_is_refused_and_zero_is_not() {
    let f = F::new();
    assert_err!(
        f.c.try_create_card(
            &f.admin,
            &Category::SaaS,
            &String::from_str(&f.e, "saas"),
            &-1,
            &0
        ),
        Error::InvalidLimit
    );
    // Zero is the resting state of a card that exists but is not yet authorised to spend, and it
    // is meaningfully different from frozen — see `limits_tests`.
    let id = f.card(Category::SaaS, "saas", 0);
    assert_eq!(f.get_card(id).limit, 0);
}

// ---------------------------------------------------------------------------
// M6 · allocation
// ---------------------------------------------------------------------------

/// T6. The guard everything downstream assumes held.
#[test]
fn allocating_past_the_unallocated_balance_is_refused() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 0);

    assert_err!(
        f.c.try_allocate(&f.admin, &card, &(101 * USD)),
        Error::OverAllocation
    );
    // Exactly the whole pool is fine — the check is `>`, not `>=`, and an off-by-one here would
    // strand the last unit of every treasury.
    f.allocate(card, 100 * USD);
    assert_eq!(f.c.get_unallocated(), 0);
    assert_eq!(f.get_card(card).balance, 100 * USD);

    // And now there is nothing left, including for a second card.
    let other = f.card(Category::AI, "ai", 0);
    assert_err!(
        f.c.try_allocate(&f.admin, &other, &1),
        Error::OverAllocation
    );
    f.check_books();
}

#[test]
fn allocation_to_a_card_that_does_not_exist_is_refused() {
    let f = F::new();
    f.deposit(100 * USD);
    assert_err!(
        f.c.try_allocate(&f.admin, &99, &(10 * USD)),
        Error::CardNotFound
    );
    f.check_books();
}

#[test]
fn deallocate_returns_money_to_the_pool() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 0);
    f.allocate(card, 60 * USD);

    f.c.deallocate(&f.admin, &card, &(25 * USD));
    assert_eq!(f.get_card(card).balance, 35 * USD);
    assert_eq!(f.c.get_unallocated(), 65 * USD);

    assert_err!(
        f.c.try_deallocate(&f.admin, &card, &(36 * USD)),
        Error::InsufficientCardBalance
    );
    f.check_books();
}

#[test]
fn rebalancing_moves_between_cards_and_nowhere_else() {
    let f = F::new();
    f.deposit(100 * USD);
    let a = f.card(Category::SaaS, "a", 0);
    let b = f.card(Category::AI, "b", 0);
    f.allocate(a, 70 * USD);

    let before = f.c.get_treasury();
    f.c.transfer_between_cards(&f.admin, &a, &b, &(30 * USD));

    assert_eq!(f.get_card(a).balance, 40 * USD);
    assert_eq!(f.get_card(b).balance, 30 * USD);
    // The treasury did not notice, and should not have: this is a move inside the allocated
    // pool, so no solvency question arises.
    assert_eq!(f.c.get_treasury(), before);
    f.check_books();
}

#[test]
fn rebalancing_is_guarded() {
    let f = F::new();
    f.deposit(100 * USD);
    let a = f.card(Category::SaaS, "a", 0);
    let b = f.card(Category::AI, "b", 0);
    f.allocate(a, 40 * USD);

    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &a, &USD),
        Error::SameCard
    );
    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &b, &(41 * USD)),
        Error::InsufficientCardBalance
    );
    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &99, &USD),
        Error::CardNotFound
    );
    assert_err!(
        f.c.try_transfer_between_cards(&f.admin, &a, &b, &0),
        Error::InvalidAmount
    );
    f.check_books();
}

// ---------------------------------------------------------------------------
// M8 · status
// ---------------------------------------------------------------------------

#[test]
fn freeze_and_unfreeze_round_trip() {
    let f = F::new();
    let id = f.card(Category::Cloud, "cloud", 0);

    f.freeze(id);
    assert_eq!(f.get_card(id).status, CardStatus::Frozen);
    f.unfreeze(id);
    assert_eq!(f.get_card(id).status, CardStatus::Active);
}

/// Freezing is not a lock on the money. An admin can still deallocate a frozen card's balance
/// back to the pool — freezing stops *spending*, and a frozen card whose funds were also stuck
/// would make freezing a decision nobody wants to take.
#[test]
fn a_frozen_card_can_still_be_deallocated() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::Cloud, "cloud", 0);
    f.allocate(id, 40 * USD);
    f.freeze(id);

    f.c.deallocate(&f.admin, &id, &(40 * USD));
    assert_eq!(f.get_card(id).balance, 0);
    assert_eq!(f.c.get_unallocated(), 100 * USD);
    f.check_books();
}
