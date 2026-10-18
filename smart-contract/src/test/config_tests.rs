//! Module M1 — construction happens once, and the contract reads its own configuration back.

use soroban_sdk::{vec, Address, Vec};

use super::harness::F;
use crate::errors::Error;
use crate::types::Role;

#[test]
fn construction_sets_config_and_seeds_roles() {
    let f = F::new();
    let cfg = f.c.get_config();

    assert_eq!(cfg.founder, f.admin);
    assert_eq!(cfg.token, f.token_id);
    // Read from the token contract, not from the deployer. A deployer who passed 6 here would
    // make every amount an explorer renders wrong by a factor of ten, with nothing on chain to
    // contradict it.
    assert_eq!(cfg.decimals, 7);

    assert_eq!(f.c.role_of(&f.admin), Some(Role::Admin));
    assert_eq!(f.c.role_of(&f.operator), Some(Role::Operator));
    assert_eq!(f.c.role_of(&f.outsider), None);

    let t = f.c.get_treasury();
    assert_eq!(t.total_deposited, 0);
    assert_eq!(t.total_allocated, 0);
    assert_eq!(t.total_settled, 0);
    f.check_books();
}

/// M1's definition of done: `initialize` cannot run twice.
///
/// # Why `initialize` can in fact never run even once
///
/// Because `__constructor` exists, the host calls it on **every** deployment — there is no way
/// to register or deploy this contract without it, which the SDK enforces by refusing a
/// deployment whose constructor arguments are missing. So by the time any transaction can reach
/// `initialize`, the contract is already configured and this is always the second call.
///
/// That is the point rather than an accident. The deck's API surface lists `initialize`, and M1
/// asks us to show that calling it a second time is refused; the constructor is what removes the
/// window between deploying and configuring in which somebody else could call it first. Keeping
/// both means the deck's entry point exists, behaves as specified, and cannot be raced. It is
/// registered as a deviation in `docs/13-deck-conformance.md`.
#[test]
fn initialize_is_always_refused_after_construction() {
    let f = F::new();
    assert_err!(
        f.c.try_initialize(
            &f.outsider,
            &f.token_id,
            &(vec![&f.e, f.outsider.clone()] as Vec<Address>)
        ),
        Error::AlreadyInitialized
    );

    // And the refusal changed nothing. The failure mode worth guarding against is not a second
    // initialize that errors — it is one that errors *after* overwriting the admin.
    assert_eq!(f.c.get_config().founder, f.admin);
    assert_eq!(f.c.role_of(&f.outsider), None);
    f.check_books();
}

/// Operators named at construction are seeded; nobody else is.
#[test]
fn constructor_seeds_every_operator_it_is_given() {
    let f = F::new();
    assert_eq!(f.c.role_of(&f.operator), Some(Role::Operator));
    assert_eq!(f.c.role_of(&f.rogue), None);
    assert_eq!(f.c.role_of(&f.settlement), None);
    // A settlement address is a destination, not a participant. Whitelisting is what authorises
    // it, and that is a different table entirely.
    assert!(!f.c.is_whitelisted(&f.settlement));
}
