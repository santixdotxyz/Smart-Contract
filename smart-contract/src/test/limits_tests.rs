//! Module M7 — the limit, the window, and the difference between being broke and being capped.
//!
//! Decision #02 lives here. The SOW never says whether a spending limit is per-transaction,
//! daily or monthly, so the period is a field on the card and the reset is lazy. These tests are
//! what pin that behaviour down.

use super::harness::{DAY, F, MONTH, USD};
use crate::errors::Error;
use crate::types::{Category, ReasonCode};

#[test]
fn spending_accumulates_within_the_window_and_only_on_approval() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::AI, "ai-tools", 50_000 * USD);
    f.allocate(card, 61_000 * USD);

    f.assert_approved(&f.spend(card, 20_000 * USD, "openai"));
    assert_eq!(f.get_card(card).spent_in_window, 20_000 * USD);
    assert_eq!(f.c.get_remaining_limit(&card), 30_000 * USD);

    f.assert_approved(&f.spend(card, 17_000 * USD, "anthropic"));
    assert_eq!(f.get_card(card).spent_in_window, 37_000 * USD);
    assert_eq!(f.c.get_remaining_limit(&card), 13_000 * USD);

    // The deck's own example: 13,000 left, 18,900 asked for.
    let declined = f.spend(card, 18_900 * USD, "openai-sub-0824");
    f.assert_declined(&declined, ReasonCode::LimitExceeded);

    // A decline does not consume limit. If it did, a card could be exhausted by requests that
    // were all refused.
    assert_eq!(f.get_card(card).spent_in_window, 37_000 * USD);
    f.check_books();
}

/// Exactly the remaining limit is approved. The check is `>`, not `>=`; an off-by-one would make
/// the last unit of every limit unusable.
#[test]
fn spending_exactly_the_remaining_limit_is_approved() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::SaaS, "saas", 50 * USD);
    f.allocate(card, 100 * USD);

    f.assert_approved(&f.spend(card, 30 * USD, "a"));
    f.assert_approved(&f.spend(card, 20 * USD, "b"));
    assert_eq!(f.c.get_remaining_limit(&card), 0);
    f.assert_declined(&f.spend(card, 1, "c"), ReasonCode::LimitExceeded);
    f.check_books();
}

/// A zero limit declines with `LIMIT_EXCEEDED`, not `CARD_FROZEN`. The two states mean different
/// things — the card is fine and the budget is not — and a console that showed them the same way
/// would send somebody to unfreeze a card that was never frozen.
#[test]
fn a_zero_limit_card_declines_on_the_limit() {
    let f = F::new();
    f.deposit(100 * USD);
    let card = f.card(Category::Other, "unbudgeted", 0);
    f.allocate(card, 100 * USD);

    f.assert_declined(&f.spend(card, 1, "anything"), ReasonCode::LimitExceeded);
    f.check_books();
}

#[test]
fn the_window_rolls_lazily_and_resets_spending() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::AI, "ai", 50_000 * USD);
    f.allocate(card, 100_000 * USD);

    f.assert_approved(&f.spend(card, 50_000 * USD, "a"));
    f.assert_declined(&f.spend(card, 1, "b"), ReasonCode::LimitExceeded);

    // Still inside the window a day before it ends.
    f.advance(MONTH - DAY);
    f.assert_declined(&f.spend(card, 1, "c"), ReasonCode::LimitExceeded);

    // Over the boundary. Nobody ran anything to make this happen — the next call notices.
    f.advance(2 * DAY);
    assert_eq!(f.c.get_remaining_limit(&card), 50_000 * USD);
    f.assert_approved(&f.spend(card, 50_000 * USD, "d"));
    f.check_books();
}

/// The window advances by whole periods rather than snapping to `now`, so reset instants stay on
/// their original cadence however long a card sits untouched.
#[test]
fn a_long_gap_advances_by_whole_windows() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::Cloud, "cloud", 1_000 * USD);
    f.allocate(card, 100_000 * USD);
    let start = f.get_card(card).window_start;

    f.assert_approved(&f.spend(card, 1_000 * USD, "a"));

    // Three and a half windows later.
    f.advance(3 * MONTH + MONTH / 2);
    f.assert_approved(&f.spend(card, 1_000 * USD, "b"));

    let c = f.get_card(card);
    assert_eq!(c.window_start, start + 3 * MONTH);
    assert_eq!(c.spent_in_window, 1_000 * USD);

    // Half a window remains, so the limit is still spent.
    f.assert_declined(&f.spend(card, 1, "c"), ReasonCode::LimitExceeded);
    f.advance(MONTH / 2 + 1);
    f.assert_approved(&f.spend(card, 1_000 * USD, "d"));
    f.check_books();
}

/// A custom window, because the deck leaves the period open and a daily card is a real thing an
/// organisation would want.
#[test]
fn a_daily_window_behaves_like_a_monthly_one() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card_windowed("daily-ads", 500 * USD, DAY);
    f.allocate(card, 10_000 * USD);

    f.assert_approved(&f.spend(card, 500 * USD, "a"));
    f.assert_declined(&f.spend(card, 1, "b"), ReasonCode::LimitExceeded);
    f.advance(DAY);
    f.assert_approved(&f.spend(card, 500 * USD, "c"));
    f.check_books();
}

/// Raising a limit takes effect immediately; it does not wait for the window.
#[test]
fn raising_the_limit_frees_spending_at_once() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(card, 10_000 * USD);

    f.assert_approved(&f.spend(card, 100 * USD, "a"));
    f.assert_declined(&f.spend(card, 1, "b"), ReasonCode::LimitExceeded);

    f.c.set_limit(&f.admin, &card, &(300 * USD), &0);
    assert_eq!(f.c.get_remaining_limit(&card), 200 * USD);
    f.assert_approved(&f.spend(card, 200 * USD, "c"));
    f.check_books();
}

/// Lowering a limit below what has already been spent leaves nothing available, and does not
/// wrap into a negative that would read as available.
#[test]
fn lowering_the_limit_below_what_is_spent_leaves_zero() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 10_000 * USD);

    f.assert_approved(&f.spend(card, 800 * USD, "a"));
    f.c.set_limit(&f.admin, &card, &(500 * USD), &0);

    assert_eq!(f.c.get_remaining_limit(&card), 0);
    f.assert_declined(&f.spend(card, 1, "b"), ReasonCode::LimitExceeded);
    f.check_books();
}

/// Changing the period does not reset what has been spent. Shortening a window that has already
/// been spent against would otherwise hand the whole limit straight back, turning a tightening
/// into a loosening.
#[test]
fn changing_the_window_does_not_forgive_spending() {
    let f = F::new();
    f.deposit(10_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 10_000 * USD);
    f.assert_approved(&f.spend(card, 900 * USD, "a"));

    f.c.set_limit(&f.admin, &card, &(1_000 * USD), &DAY);
    let c = f.get_card(card);
    assert_eq!(c.limit_window_secs, DAY);
    assert_eq!(c.spent_in_window, 900 * USD);
    f.assert_declined(&f.spend(card, 200 * USD, "b"), ReasonCode::LimitExceeded);
    f.check_books();
}

#[test]
fn limit_changes_are_guarded() {
    let f = F::new();
    let card = f.card(Category::SaaS, "saas", 0);
    assert_err!(
        f.c.try_set_limit(&f.admin, &card, &-1, &0),
        Error::InvalidLimit
    );
    assert_err!(
        f.c.try_set_limit(&f.admin, &99, &(10 * USD), &0),
        Error::CardNotFound
    );
    // `0` for the window means "leave it alone" rather than "zero seconds", which would make the
    // limit meaningless by resetting on every call.
    f.c.set_limit(&f.admin, &card, &(10 * USD), &0);
    assert_eq!(
        f.get_card(card).limit_window_secs,
        crate::cards::DEFAULT_WINDOW_SECS
    );
}

/// Balance and limit are two independent constraints, and this is the case that proves it: the
/// card is rich and capped at the same time.
#[test]
fn a_card_can_be_rich_and_out_of_limit() {
    let f = F::new();
    f.deposit(100_000 * USD);
    let card = f.card(Category::AI, "ai", 1_000 * USD);
    f.allocate(card, 61_000 * USD);

    f.assert_approved(&f.spend(card, 1_000 * USD, "a"));
    assert_eq!(f.get_card(card).balance, 61_000 * USD); // untouched: authorization moves no money
    f.assert_declined(&f.spend(card, 1, "b"), ReasonCode::LimitExceeded);
    f.check_books();
}
