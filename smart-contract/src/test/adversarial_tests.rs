//! The cases that were found by attacking the contract rather than by describing it.
//!
//! Every other file in this suite was written from the specification: it asserts that the
//! contract does what it was asked to do. This one was written the other way round — by looking
//! for inputs the specification never mentions and seeing what broke. **Five real bugs came out
//! of it**, and the tests that found them are kept here so they cannot come back:
//!
//! | Test | The bug it found |
//! |---|---|
//! | [`a_huge_limit_window_is_refused`] | `create_card(…, u64::MAX)` was accepted, then **trapped on every read of the card** — permanently, with no route back |
//! | [`a_huge_window_through_set_limit_is_refused`] | the same bug through a second door, on a card that was already working |
//! | [`a_missing_settlement_says_so`] | `get_settlement` answered `SpendNotFound` — a claim about a different ledger |
//! | [`the_contract_cannot_be_its_own_settlement_destination`] | settling to the contract stranded the amount: off the card, never back to `unallocated`, unreachable |
//! | [`the_fixture_token_does_not_mint_on_a_self_transfer`] | `mock-token` **created tokens** on a self-transfer, and it is the token `check_books` measures the contract against |
//!
//! The rest are properties that were already true and are worth pinning down, because they are
//! the ones whose failure would be silent.
//!
//! # What made them findable
//!
//! Nothing here is exotic. Four of the five are an argument at the edge of its type — `u64::MAX`,
//! an address that happens to be the contract's own, a 65th byte — and the fifth is an aliasing
//! case (`from == to`) that only appears when two names refer to one thing. Those are the shapes
//! worth trying first on anything new added to this contract.

use soroban_sdk::String;

use super::harness::{DAY, F, USD};
use crate::errors::Error;
use crate::types::{CardStatus, Category, ReasonCode};

// ===========================================================================
// The window bound — a live bug, twice over
// ===========================================================================

/// A limit window so long that `window_start + limit_window_secs` leaves the range of a `u64`.
///
/// # What this used to do
///
/// `check_window` refused only zero, so the card was created happily. From then on every call
/// that rolled the window trapped with `attempt to add with overflow` — `authorize_spend`,
/// `set_limit`, `get_card_live`, `get_remaining_limit`. The card could not spend, and its limit
/// could not be corrected either, because correcting it also rolls the window.
///
/// One mistyped argument bricked a card permanently, with no route back and no upgrade path.
#[test]
fn a_huge_limit_window_is_refused() {
    let f = F::new();
    assert_err!(
        f.c.try_create_card(
            &f.admin,
            &Category::Other,
            &String::from_str(&f.e, "x"),
            &(100 * USD),
            &u64::MAX
        ),
        Error::InvalidWindow
    );
    // And nothing was created on the way to being refused.
    assert_eq!(f.c.list_cards().len(), 0);
}

/// The same bug through `set_limit`, which is the door that would have hurt more: it takes a card
/// that is working and makes it unreadable.
#[test]
fn a_huge_window_through_set_limit_is_refused() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 1_000 * USD);

    assert_err!(
        f.c.try_set_limit(&f.admin, &id, &(555 * USD), &u64::MAX),
        Error::InvalidWindow
    );

    // The refusal changed nothing — not the window it was rejected for, and not the limit it
    // would otherwise have set on the way past.
    let c = f.get_card(id);
    assert_eq!(
        c.limit,
        100 * USD,
        "a rejected set_limit must not persist the new limit"
    );
    assert_eq!(c.limit_window_secs, crate::cards::DEFAULT_WINDOW_SECS);

    // And the card still works.
    assert_eq!(f.c.get_remaining_limit(&id), 100 * USD);
    f.assert_approved(&f.spend(id, 100 * USD, "still-alive"));
    f.check_books();
}

/// Exactly at the bound is legal; one second past it is not.
#[test]
fn the_window_bound_is_inclusive() {
    let f = F::new();
    let max = crate::cards::MAX_WINDOW_SECS;

    let id =
        f.c.create_card(
            &f.admin,
            &Category::Other,
            &String::from_str(&f.e, "decade"),
            &(10 * USD),
            &max,
        )
        .id;
    assert_eq!(f.get_card(id).limit_window_secs, max);
    // Readable, which is the whole point.
    assert_eq!(f.c.get_remaining_limit(&id), 10 * USD);

    assert_err!(
        f.c.try_create_card(
            &f.admin,
            &Category::Other,
            &String::from_str(&f.e, "x"),
            &0,
            &(max + 1)
        ),
        Error::InvalidWindow
    );
}

/// The shortest legal window. One second is absurd as a budget and must still behave.
#[test]
fn a_one_second_window_rolls() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let id = f.card_windowed("per-second", 10 * USD, 1);
    f.allocate(id, 1_000 * USD);

    f.assert_approved(&f.spend(id, 10 * USD, "a"));
    f.assert_declined(&f.spend(id, 1, "b"), ReasonCode::LimitExceeded);
    f.advance(1);
    f.assert_approved(&f.spend(id, 10 * USD, "c"));
    f.check_books();
}

// ===========================================================================
// Error codes that name the right thing
// ===========================================================================

/// `get_settlement` on an id no settlement has.
///
/// It used to answer `SpendNotFound`, which is a statement about a different ledger — and would
/// send somebody looking for a spend that exists perfectly well.
#[test]
fn a_missing_settlement_says_so() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 100 * USD);
    let spend = f.spend(id, 10 * USD, "x");

    // Spend 1 exists. Settlement 1 does not.
    assert!(f.c.try_get_spend(&spend.id).is_ok());
    assert_err!(f.c.try_get_settlement(&1), Error::SettlementNotFound);
    assert_err!(f.c.try_get_settlement(&999), Error::SettlementNotFound);
    f.check_books();
}

// ===========================================================================
// The property everything else rests on: a failed transfer takes the writes with it
// ===========================================================================

/// `deposit` writes the treasury and **then** transfers. If the transfer fails, the books must
/// not keep the deposit.
///
/// This is the ten-step discipline's step 9 being load-bearing. If the host did not revert, the
/// treasury would be permanently overstated with no bug anywhere in this contract — and I2 would
/// then decline every spend forever.
#[test]
fn a_failed_deposit_transfer_takes_the_books_with_it() {
    let f = F::new();
    let poor = f.rogue.clone(); // holds no tokens; mock-token panics on an overdraft

    assert!(f.c.try_deposit(&poor, &(1_000 * USD)).is_err());

    assert_eq!(f.c.get_treasury().total_deposited, 0);
    assert_eq!(f.held(), 0);
    f.check_books();
}

/// The same for `settle`, which writes four things before it transfers.
///
/// Driven by clawing the contract dry so the token cannot pay while the contract's own books
/// still believe it is solvent — the one situation where every write succeeds and the transfer
/// cannot.
#[test]
fn a_failed_settlement_transfer_takes_everything_with_it() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 100 * USD);
    f.whitelist(&f.settlement);
    let r = f.spend(id, 50 * USD, "x");
    f.assert_approved(&r);

    f.clawback(100 * USD);

    assert!(f
        .c
        .try_settle(&f.operator, &r.id, &f.settlement, &(50 * USD))
        .is_err());

    assert_eq!(
        f.get_card(id).balance,
        100 * USD,
        "the card must not be debited"
    );
    assert_eq!(
        f.c.get_treasury().total_settled,
        0,
        "total_settled must not move"
    );
    assert!(
        !f.c.get_spend(&r.id).settled,
        "the spend must not be marked settled"
    );
    assert!(f.c.get_settlement_of_spend(&r.id).is_none());
    assert_eq!(f.c.list_settlements().len(), 0);
}

// ===========================================================================
// Boundaries the specification states but nothing exercised at the edge
// ===========================================================================

/// A merchant reference of exactly 64 bytes is allowed; 65 is not.
///
/// The bound is on the record a reviewer reads, so being one byte wrong in the generous direction
/// bloats every spend entry and one byte wrong in the strict direction rejects legitimate
/// references that a gateway has no way to shorten.
#[test]
fn the_merchant_reference_bound_is_inclusive() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 100 * USD);

    let at = String::from_str(&f.e, &"a".repeat(64));
    let past = String::from_str(&f.e, &"a".repeat(65));

    assert!(f.c.try_authorize_spend(&f.operator, &id, &USD, &at).is_ok());
    assert_err!(
        f.c.try_authorize_spend(&f.operator, &id, &USD, &past),
        Error::InvalidMerchantRef
    );
    f.check_books();
}

/// 32 bytes of label is allowed; 33 is not.
#[test]
fn the_label_bound_is_inclusive() {
    let f = F::new();
    let at = String::from_str(&f.e, &"a".repeat(32));
    let past = String::from_str(&f.e, &"a".repeat(33));

    assert!(f
        .c
        .try_create_card(&f.admin, &Category::SaaS, &at, &0, &0)
        .is_ok());
    assert_err!(
        f.c.try_create_card(&f.admin, &Category::SaaS, &past, &0, &0),
        Error::InvalidLabel
    );
}

/// An effectively unbounded limit does not overflow `spent_in_window` as approvals accumulate.
///
/// # And it shows the race window from the limit's side
///
/// With `limit = i128::MAX` the limit never bites, and **authorization does not debit the
/// balance** — so the same 1,000 can be approved over and over. `spent_in_window` climbs past the
/// card's balance and nothing objects, because nothing has been paid yet.
///
/// That is decision #01 working as designed rather than a hole: the money is stopped at
/// settlement, where the balance is checked again. The last two assertions are the proof.
#[test]
fn an_unbounded_limit_accumulates_without_overflowing() {
    let f = F::new();
    f.deposit(1_000 * USD);
    f.whitelist(&f.settlement);
    let id =
        f.c.create_card(
            &f.admin,
            &Category::Other,
            &String::from_str(&f.e, "unbounded"),
            &i128::MAX,
            &0,
        )
        .id;
    f.allocate(id, 1_000 * USD);

    let a = f.spend(id, 1_000 * USD, "a");
    let b = f.spend(id, 1_000 * USD, "b");
    let c = f.spend(id, 1_000 * USD, "c");
    f.assert_approved(&a);
    f.assert_approved(&b);
    f.assert_approved(&c);

    // Three approvals against a card that can only cover one, and the counter is exact.
    assert_eq!(f.get_card(id).spent_in_window, 3_000 * USD);
    assert_eq!(
        f.get_card(id).balance,
        1_000 * USD,
        "authorization moves nothing"
    );

    // Settlement is where it stops. The first one is paid; the others cannot be.
    f.settle(a.id, 1_000 * USD);
    assert_err!(
        f.c.try_settle(&f.operator, &b.id, &f.settlement, &(1_000 * USD)),
        Error::InsufficientCardBalance
    );
    assert_eq!(f.token.balance(&f.settlement), 1_000 * USD);
    f.check_books();
}

// ===========================================================================
// States the happy path never visits
// ===========================================================================

/// An admin can pull the money out from under an approved spend, and the settlement then fails
/// cleanly rather than going negative.
///
/// This is the race window's other shape: not two spends competing, but an allocation decision
/// overtaking one. The contract behaves the same way — it refuses to pay what the card no longer
/// has, and the orphan stays visible.
#[test]
fn deallocating_under_an_approved_spend_refuses_the_settlement() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 100 * USD);
    f.whitelist(&f.settlement);

    let r = f.spend(id, 50 * USD, "x");
    f.assert_approved(&r);
    f.c.deallocate(&f.admin, &id, &(100 * USD));

    assert_err!(
        f.c.try_settle(&f.operator, &r.id, &f.settlement, &(50 * USD)),
        Error::InsufficientCardBalance
    );
    // Still approved, still unsettled, still visible.
    assert!(!f.c.get_spend(&r.id).settled);
    f.check_books();
}

/// A window that lapses while the card is frozen still rolls, and the roll is **persisted** even
/// though the spend that triggered it was declined.
///
/// If the roll were only applied in memory on a declined path, the card would come out of a
/// freeze still carrying last period's spending and would decline for the wrong reason.
#[test]
fn a_window_rolls_while_the_card_is_frozen() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let id = f.card(Category::SaaS, "saas", 10 * USD);
    f.allocate(id, 1_000 * USD);

    f.assert_approved(&f.spend(id, 10 * USD, "a"));
    f.freeze(id);
    f.advance(40 * DAY);
    f.assert_declined(&f.spend(id, USD, "b"), ReasonCode::CardFrozen);

    assert_eq!(
        f.get_card(id).spent_in_window,
        0,
        "the roll must persist even when the spend that triggered it was declined"
    );

    f.unfreeze(id);
    f.assert_approved(&f.spend(id, 10 * USD, "c"));
    f.check_books();
}

/// Setting a status to the value it already holds is allowed and changes nothing.
///
/// Unlike the whitelist, where a no-op is refused: there a script reporting success for a change
/// that did not happen would be misleading evidence, whereas re-freezing an already-frozen card
/// is a perfectly reasonable thing for an operator reacting to an incident to do twice.
#[test]
fn setting_a_status_to_what_it_already_is_is_allowed() {
    let f = F::new();
    let id = f.card(Category::Cloud, "cloud", 0);

    f.c.set_card_status(&f.admin, &id, &CardStatus::Active);
    assert_eq!(f.get_card(id).status, CardStatus::Active);
    f.freeze(id);
    f.c.set_card_status(&f.operator, &id, &CardStatus::Frozen);
    assert_eq!(f.get_card(id).status, CardStatus::Frozen);
}

// ===========================================================================
// The contract as its own settlement destination
// ===========================================================================

/// Whitelisting the treasury's own address, and settling to it.
///
/// # What this used to do
///
/// The transfer is contract → contract, a no-op — but every write around it still ran. The card
/// was debited and `total_settled` rose, so the amount left the card and **never returned to
/// `unallocated`**. The tokens stayed in the contract, accounted to nobody, out of reach of every
/// entry point. Not stolen; stranded, permanently.
///
/// It is an ordinary mistake to make: a contract id and a settlement address are both Stellar
/// strkeys and look alike in a config file.
#[test]
fn the_contract_cannot_be_its_own_settlement_destination() {
    let f = F::new();
    f.deposit(100 * USD);
    let id = f.card(Category::SaaS, "saas", 100 * USD);
    f.allocate(id, 100 * USD);

    // Refused where the mistake is made …
    assert_err!(
        f.c.try_add_whitelist(&f.admin, &f.contract_id),
        Error::InvalidDestination
    );
    assert!(!f.c.is_whitelisted(&f.contract_id));

    // … and again where the money would move, so a whitelist written before this guard existed
    // cannot spend through it.
    f.whitelist(&f.settlement);
    let r = f.spend(id, 50 * USD, "self");
    f.assert_approved(&r);
    assert_err!(
        f.c.try_settle(&f.operator, &r.id, &f.contract_id, &(50 * USD)),
        Error::InvalidDestination
    );

    assert_eq!(f.get_card(id).balance, 100 * USD);
    assert_eq!(f.c.get_treasury().total_settled, 0);
    f.check_books();
}

/// The fixture token must not create tokens when an address pays itself.
///
/// # Why a bug in a fixture is worth a test
///
/// `transfer` read both balances and then wrote both; with `from == to` that reads the same
/// number twice and the second write wins, so `transfer(x, x, n)` minted `n`.
///
/// Anybody can `mint` this token, so the minting was not the harm. The harm is that
/// [`F::check_books`] — the assertion every test in this suite ends with — compares the
/// treasury's accounting against *this token's* balances. A fixture that quietly inflates is a
/// fixture that can hide a real accounting bug in the contract under test.
#[test]
fn the_fixture_token_does_not_mint_on_a_self_transfer() {
    let f = F::new();
    let before = f.token.balance(&f.funder);
    f.token.transfer(&f.funder, &f.funder, &(500 * USD));
    assert_eq!(
        f.token.balance(&f.funder),
        before,
        "a self-transfer must not create tokens"
    );

    // And the contract paying itself would likewise not have inflated it.
    f.deposit(100 * USD);
    f.token
        .transfer(&f.contract_id, &f.contract_id, &(100 * USD));
    assert_eq!(f.held(), 100 * USD);
    f.check_books();
}
