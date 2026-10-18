//! Long randomised sequences, with the books checked after every single step.
//!
//! # What this catches that the scenario tests do not
//!
//! Every other test in the suite asserts something a person thought of. This one asserts the
//! invariants after an operation order nobody chose — deallocate after a settlement, a rebalance
//! between two cards that have both been spent against, a window rolling in the middle of a
//! sequence. Accounting bugs tend to live in orderings rather than in operations, and an
//! ordering is the one thing a hand-written scenario cannot enumerate.
//!
//! # No external crate, and no real randomness
//!
//! `proptest` does not build for a `no_std` Soroban crate, and `Math::random` has no meaning
//! inside a deterministic host. A small xorshift generator seeded from a constant gives the
//! same thing that matters: an ordering nobody chose, reproduced exactly on every run and on
//! every machine. A failure here is a failure anyone can reproduce by running the suite, which
//! is worth more than an unreproducible one found faster.

use super::harness::{DAY, F, USD};
use crate::types::{CardStatus, Category, Decision};

/// xorshift64*. Deterministic, seeded per test, and entirely adequate for choosing which of six
/// operations to try next.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Two hundred operations chosen at random against four cards, checking the books after each.
///
/// Nothing here asserts an outcome. Any individual call may legitimately be refused — an
/// over-allocation, a settle on an already-settled spend — and being refused is as interesting
/// as succeeding. The assertion is that **whatever happens, the books balance afterwards**, which
/// is the property the whole design rests on.
#[test]
fn the_books_balance_after_any_sequence_of_operations() {
    for seed in [1u64, 7, 42, 1337, 99_991] {
        run_sequence(seed, 200);
    }
}

fn run_sequence(seed: u64, steps: usize) {
    let f = F::new();
    let mut rng = Rng(seed);

    f.deposit(1_000_000 * USD);
    f.whitelist(&f.settlement);

    let cards = [
        f.card(Category::SaaS, "saas", 50_000 * USD),
        f.card(Category::AI, "ai", 50_000 * USD),
        f.card(Category::Advertising, "ads", 50_000 * USD),
        f.card(Category::Cloud, "cloud", 50_000 * USD),
    ];
    for c in cards {
        f.allocate(c, 100_000 * USD);
    }
    f.check_books();

    for _ in 0..steps {
        let card = cards[rng.below(4) as usize];
        let other = cards[rng.below(4) as usize];
        // Amounts span three orders of magnitude, so the sequence hits both "trivially fine" and
        // "just past a limit" without either dominating.
        let amount = (rng.below(80_000) + 1) as i128 * USD / 10;

        match rng.below(9) {
            0 => {
                let _ = f.c.try_deposit(&f.funder, &amount);
            }
            1 => {
                let _ = f.c.try_allocate(&f.admin, &card, &amount);
            }
            2 => {
                let _ = f.c.try_deallocate(&f.admin, &card, &amount);
            }
            3 => {
                let _ =
                    f.c.try_transfer_between_cards(&f.admin, &card, &other, &amount);
            }
            4 => {
                let _ = f.c.try_set_limit(&f.admin, &card, &amount, &0);
            }
            5 => {
                let status = if rng.below(2) == 0 {
                    CardStatus::Frozen
                } else {
                    CardStatus::Active
                };
                let _ = f.c.try_set_card_status(&f.admin, &card, &status);
            }
            6 => {
                let _ = f.c.try_authorize_spend(
                    &f.operator,
                    &card,
                    &amount,
                    &soroban_sdk::String::from_str(&f.e, "rnd"),
                );
            }
            7 => {
                // Settle whatever approved-and-unsettled record is oldest, for whatever it was
                // authorised for. Picking the oldest rather than a random one means settlements
                // actually happen instead of mostly bouncing off `AlreadySettled`.
                if let Some(s) =
                    f.c.list_spends(&0)
                        .iter()
                        .find(|s| s.decision == Decision::Approved && !s.settled)
                {
                    let _ = f.c.try_settle(&f.operator, &s.id, &f.settlement, &s.amount);
                }
            }
            _ => f.advance(DAY * (rng.below(45) + 1)),
        }

        f.check_books();
    }

    // The sequence has to have done something, or the test is a very slow no-op. Every seed must
    // produce at least one settlement — money actually leaving is the operation with the most
    // ways to go wrong.
    assert!(
        !f.c.list_settlements().is_empty(),
        "seed {seed} settled nothing — the sequence is not exercising the interesting path"
    );
}

/// A card driven to exactly zero and back, repeatedly. The state where a balance is zero but the
/// card still exists is where an `unwrap_or(0)` and a genuine zero become indistinguishable.
#[test]
fn a_card_emptied_and_refilled_many_times_stays_consistent() {
    let f = F::new();
    f.deposit(1_000 * USD);
    f.whitelist(&f.settlement);
    let card = f.card(Category::Other, "cycle", 1_000_000 * USD);

    for _ in 0..25 {
        f.allocate(card, 40 * USD);
        let r = f.spend(card, 40 * USD, "cycle");
        f.assert_approved(&r);
        f.settle(r.id, 40 * USD);
        assert_eq!(f.get_card(card).balance, 0);
        f.check_books();
    }

    assert_eq!(f.c.get_treasury().total_settled, 1_000 * USD);
    assert_eq!(f.held(), 0);
    f.check_books();
}
