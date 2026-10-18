//! The suite. Organised by the sections of `docs/10-testing.md`.
//!
//! Every scenario ends with [`harness::F::check_books`], which asserts the accounting identity by
//! **summing the actual cards** and compares the result against the token's **real** balance.
//! The invariant check inside the contract can do neither: it reads three running totals that
//! stay perfectly consistent with each other even when a card was never credited, and it cannot
//! see the token at all. A missing transfer hides in exactly that gap.

#[macro_use]
mod harness;

mod acceptance_tests;
mod adversarial_tests;
mod cards_tests;
mod config_tests;
mod decimals_tests;
mod deck_conformance;
mod demo_replay;
mod limits_tests;
mod property_tests;
mod rebuild_test;
mod roles_tests;
mod settle_tests;
mod spend_tests;
mod treasury_tests;
