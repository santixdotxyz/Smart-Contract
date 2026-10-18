//! The fixture every test builds on, and the book-check every scenario ends with.
//!
//! # What `check_books` proves that the contract cannot
//!
//! The contract's own [`crate::invariant::assert_books`] reads three running totals. They are
//! internally consistent by construction: if `allocate` raised `total_allocated` and forgot to
//! credit the card, the totals would still agree with each other and the assertion would pass.
//! And no assertion inside the contract can see the token's real balance, so a `settle` that
//! updated every record and never transferred looks perfect from the inside.
//!
//! `check_books` closes both gaps. It sums the **actual cards** and compares the token's
//! **actual balance**. Those two comparisons are where a missing credit and a missing transfer
//! respectively have to show up.

#![allow(dead_code)]

use core::cell::Cell;

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{vec, Address, Env, String, Vec};

use crate::types::{Card, CardStatus, Category, Decision, ReasonCode, Settlement, SpendRecord};
use crate::{SaintixTreasury, SaintixTreasuryClient};

/// One unit of a 7-decimal token, which is what USDC is on Stellar. Every figure in the tests is
/// written as a multiple of this rather than as a raw integer, so a misplaced zero is visible.
pub const USD: i128 = 10_000_000;

pub const DAY: u64 = 24 * 60 * 60;
pub const MONTH: u64 = 30 * DAY;

/// Assert a client `try_*` call was refused with an exact error code.
///
/// Exactness is the point. `is_err()` would pass for any of the twenty codes, and both the demo
/// script and the indexer branch on the number — a test that cannot tell 6 from 8 does not test
/// the thing that matters.
#[macro_export]
macro_rules! assert_err {
    ($call:expr, $err:expr) => {
        assert_eq!($call, Err(Ok($err)), "expected {:?}", $err)
    };
}

pub struct F<'a> {
    pub e: Env,
    pub c: SaintixTreasuryClient<'a>,
    pub token: mock_token::MockTokenClient<'a>,
    pub token_id: Address,
    pub contract_id: Address,

    pub admin: Address,
    pub admin2: Address,
    pub operator: Address,
    pub outsider: Address,
    pub settlement: Address,
    pub rogue: Address,
    pub funder: Address,

    /// Tokens removed from the contract behind its back, by [`F::clawback`]. `check_books` has
    /// to know about them: after a clawback the token balance is legitimately short of the
    /// obligation, and that is the state test T5 exists to exercise rather than a bug.
    clawed: Cell<i128>,
}

impl F<'_> {
    /// A treasury with a 7-decimal token, one admin, one operator, and nothing deposited.
    pub fn new() -> Self {
        Self::with_decimals(7)
    }

    pub fn with_decimals(decimals: u32) -> Self {
        let e = Env::default();
        e.mock_all_auths();

        // A default `Env` starts at ledger 0, timestamp 0. Both are values the real network can
        // never produce, and both hide bugs: a record stamped `ledger_seq: 0` is
        // indistinguishable from one whose field was never set, and a card created at timestamp
        // 0 has a limit window that started at the Unix epoch. Starting somewhere plausible
        // makes "the field was populated" a real assertion.
        e.ledger().with_mut(|l| {
            l.sequence_number = 1_000_000;
            l.timestamp = 1_756_000_000; // 2025-08-24, when the deck was written
        });

        let admin = Address::generate(&e);
        let admin2 = Address::generate(&e);
        let operator = Address::generate(&e);
        let outsider = Address::generate(&e);
        let settlement = Address::generate(&e);
        let rogue = Address::generate(&e);
        let funder = Address::generate(&e);

        let token_id = e.register(mock_token::MockToken, (decimals,));
        let token = mock_token::MockTokenClient::new(&e, &token_id);

        let contract_id = e.register(
            SaintixTreasury,
            (
                admin.clone(),
                token_id.clone(),
                vec![&e, operator.clone()] as Vec<Address>,
            ),
        );
        let c = SaintixTreasuryClient::new(&e, &contract_id);

        // The funder starts rich. Nothing in the suite depends on the figure; it only has to be
        // larger than any scenario spends, so that "not enough money" is never an accident.
        token.mint(&funder, &(100_000_000 * USD));

        F {
            e,
            c,
            token,
            token_id,
            contract_id,
            admin,
            admin2,
            operator,
            outsider,
            settlement,
            rogue,
            funder,
            clawed: Cell::new(0),
        }
    }

    // -----------------------------------------------------------------------
    // Shorthands
    // -----------------------------------------------------------------------

    pub fn deposit(&self, amount: i128) {
        self.c.deposit(&self.funder, &amount);
    }

    /// A card with a limit and the default 30-day window.
    pub fn card(&self, category: Category, label: &str, limit: i128) -> u32 {
        self.c
            .create_card(
                &self.admin,
                &category,
                &String::from_str(&self.e, label),
                &limit,
                &0,
            )
            .id
    }

    pub fn card_windowed(&self, label: &str, limit: i128, window: u64) -> u32 {
        self.c
            .create_card(
                &self.admin,
                &Category::Other,
                &String::from_str(&self.e, label),
                &limit,
                &window,
            )
            .id
    }

    pub fn allocate(&self, card_id: u32, amount: i128) {
        self.c.allocate(&self.admin, &card_id, &amount);
    }

    pub fn freeze(&self, card_id: u32) {
        self.c
            .set_card_status(&self.admin, &card_id, &CardStatus::Frozen);
    }

    pub fn unfreeze(&self, card_id: u32) {
        self.c
            .set_card_status(&self.admin, &card_id, &CardStatus::Active);
    }

    pub fn spend(&self, card_id: u32, amount: i128, reference: &str) -> SpendRecord {
        self.c.authorize_spend(
            &self.operator,
            &card_id,
            &amount,
            &String::from_str(&self.e, reference),
        )
    }

    pub fn whitelist(&self, who: &Address) {
        self.c.add_whitelist(&self.admin, who);
    }

    pub fn settle(&self, spend_id: u32, amount: i128) -> Settlement {
        self.c
            .settle(&self.operator, &spend_id, &self.settlement, &amount)
    }

    pub fn get_card(&self, card_id: u32) -> Card {
        self.c.get_card(&card_id)
    }

    pub fn held(&self) -> i128 {
        self.token.balance(&self.contract_id)
    }

    /// Move the ledger clock forward. Sequence advances too, so `ledger_seq` on records written
    /// after a jump does not collide with ones written before it.
    pub fn advance(&self, secs: u64) {
        self.e.ledger().with_mut(|l| {
            l.timestamp += secs;
            l.sequence_number += (secs / 5) as u32 + 1;
        });
    }

    /// Take tokens out of the contract without its knowledge — an issuer clawback.
    ///
    /// The only honest route to `TREASURY_INSOLVENT`. Reaching that state by writing storage
    /// directly would prove nothing about the contract; this proves the check reads the token
    /// rather than its own books.
    pub fn clawback(&self, amount: i128) {
        self.token.clawback(&self.contract_id, &amount);
        self.clawed.set(self.clawed.get() + amount);
    }

    // -----------------------------------------------------------------------
    // The book check
    // -----------------------------------------------------------------------

    /// Assert every accounting property that can be checked from outside. Call it at the end of
    /// every scenario — it is cheap, and it is what catches the errors a single assertion about
    /// one balance never will.
    pub fn check_books(&self) {
        let t = self.c.get_treasury();

        // I1, the half the contract cannot check: the running total against the actual cards.
        let summed: i128 = self.c.list_cards().iter().map(|c| c.balance).sum();
        assert_eq!(
            summed,
            t.total_allocated - t.total_settled,
            "I1: Σ card.balance ≠ total_allocated − total_settled"
        );

        // I1, the cheap half, restated from outside.
        let unallocated = t.total_deposited - t.total_allocated;
        assert!(unallocated >= 0, "I1: unallocated went negative");
        assert!(summed >= 0, "I1: a card balance went negative");

        // The deck's stated invariant, in its own words.
        assert_eq!(
            summed + unallocated,
            t.total_deposited - t.total_settled,
            "deck invariant: Σ card.balance + unallocated ≠ what the treasury holds"
        );

        // I2, the half no on-chain assertion can make: the token's real balance. Anything
        // deliberately clawed back is subtracted, because that gap is the point of T5 rather
        // than a bug.
        assert_eq!(
            self.held(),
            t.total_deposited - t.total_settled - self.clawed.get(),
            "I2: the contract does not hold what its books say it owes"
        );

        // I3: no spend has two settlements, and every settled record has one.
        for s in self.c.list_spends(&0).iter() {
            let found = self.c.get_settlement_of_spend(&s.id);
            assert_eq!(
                s.settled,
                found.is_some(),
                "I3: spend {} settled flag disagrees with the settlement index",
                s.id
            );
        }
        let mut seen: Vec<u32> = Vec::new(&self.e);
        for s in self.c.list_settlements().iter() {
            assert!(
                !seen.contains(s.spend_id),
                "I3: spend {} settled twice",
                s.spend_id
            );
            seen.push_back(s.spend_id);
        }
    }

    // -----------------------------------------------------------------------
    // Assertions used across several files
    // -----------------------------------------------------------------------

    pub fn assert_approved(&self, r: &SpendRecord) {
        assert_eq!(r.decision, Decision::Approved, "expected an approval");
        assert_eq!(r.reason_code, ReasonCode::None);
        assert!(!r.settled);
    }

    pub fn assert_declined(&self, r: &SpendRecord, reason: ReasonCode) {
        assert_eq!(r.decision, Decision::Declined, "expected a decline");
        assert_eq!(r.reason_code, reason, "wrong reason code");
        assert!(!r.settled);
    }
}
