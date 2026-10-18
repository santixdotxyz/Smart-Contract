//! Module M2 — grant, revoke, and the guard that keeps the contract usable.

use super::harness::{F, USD};
use crate::errors::Error;
use crate::types::{Category, Role};

#[test]
fn grant_and_revoke() {
    let f = F::new();

    f.c.grant_role(&f.admin, &f.admin2, &Role::Admin);
    assert_eq!(f.c.role_of(&f.admin2), Some(Role::Admin));

    f.c.revoke_role(&f.admin, &f.admin2);
    assert_eq!(f.c.role_of(&f.admin2), None);
}

#[test]
fn only_an_admin_may_change_roles() {
    let f = F::new();

    assert_err!(
        f.c.try_grant_role(&f.outsider, &f.outsider, &Role::Admin),
        Error::Unauthorized
    );
    // And an operator is not a lesser admin. The roles guard different things — the shape of the
    // money versus its movement — so neither implies the other.
    assert_err!(
        f.c.try_grant_role(&f.operator, &f.outsider, &Role::Operator),
        Error::Unauthorized
    );
}

/// T7's family: every admin-only entry point refuses a caller who is merely authenticated.
#[test]
fn admin_only_entry_points_refuse_an_operator() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);

    assert_err!(
        f.c.try_create_card(
            &f.operator,
            &Category::AI,
            &soroban_sdk::String::from_str(&f.e, "ai"),
            &0,
            &0
        ),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_allocate(&f.operator, &card, &(10 * USD)),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_deallocate(&f.operator, &card, &(10 * USD)),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_set_limit(&f.operator, &card, &(10 * USD), &0),
        Error::Unauthorized
    );
    assert_err!(
        f.c.try_add_whitelist(&f.operator, &f.settlement),
        Error::Unauthorized
    );
    f.check_books();
}

/// T8's family: the operator-only entry points refuse an admin.
#[test]
fn operator_only_entry_points_refuse_an_admin() {
    let f = F::new();
    f.deposit(1_000 * USD);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 500 * USD);

    assert_err!(
        f.c.try_authorize_spend(
            &f.admin,
            &card,
            &(10 * USD),
            &soroban_sdk::String::from_str(&f.e, "ref")
        ),
        Error::Unauthorized
    );
    f.check_books();
}

/// Either role may freeze. An admin freezes as policy; an operator freezes because something is
/// happening now, and making them wait for an admin is the wrong trade in the one case where
/// speed is the entire point.
#[test]
fn either_role_may_freeze_a_card() {
    let f = F::new();
    let card = f.card(Category::Cloud, "cloud", 0);

    f.c.set_card_status(&f.operator, &card, &crate::types::CardStatus::Frozen);
    f.c.set_card_status(&f.admin, &card, &crate::types::CardStatus::Active);

    assert_err!(
        f.c.try_set_card_status(&f.outsider, &card, &crate::types::CardStatus::Frozen),
        Error::Unauthorized
    );
}

/// The guard that stops the contract from being bricked.
///
/// There is no upgrade path and no recovery address. A contract with no admin can never create a
/// card, allocate, or manage the whitelist again — the money is not stolen, it is stranded, and
/// nothing can be done about it afterwards.
#[test]
fn the_last_admin_cannot_be_revoked() {
    let f = F::new();
    assert_err!(f.c.try_revoke_role(&f.admin, &f.admin), Error::LastAdmin);
    assert_eq!(f.c.role_of(&f.admin), Some(Role::Admin));

    // With a second admin in place the first may step down.
    f.c.grant_role(&f.admin, &f.admin2, &Role::Admin);
    f.c.revoke_role(&f.admin, &f.admin);
    assert_eq!(f.c.role_of(&f.admin), None);

    // And now the second is the last one.
    assert_err!(f.c.try_revoke_role(&f.admin2, &f.admin2), Error::LastAdmin);
}

/// Revoking the last *operator* is allowed — a treasury with no operator cannot authorise or
/// settle, but an admin can appoint a new one. It is recoverable, so it is not guarded.
#[test]
fn the_last_operator_may_be_revoked() {
    let f = F::new();
    f.c.revoke_role(&f.admin, &f.operator);
    assert_eq!(f.c.role_of(&f.operator), None);

    f.c.grant_role(&f.admin, &f.operator, &Role::Operator);
    assert_eq!(f.c.role_of(&f.operator), Some(Role::Operator));
}

#[test]
fn revoking_an_address_that_holds_nothing_is_refused() {
    let f = F::new();
    assert_err!(
        f.c.try_revoke_role(&f.admin, &f.outsider),
        Error::RoleNotFound
    );
}

/// The founder address in `Config` is a record of who deployed, not a standing privilege. It is
/// an ordinary admin and can be revoked like any other.
#[test]
fn the_founder_holds_no_privilege_beyond_its_role() {
    let f = F::new();
    f.c.grant_role(&f.admin, &f.admin2, &Role::Admin);
    f.c.revoke_role(&f.admin2, &f.admin);

    assert_eq!(f.c.get_config().founder, f.admin);
    assert_err!(
        f.c.try_create_card(
            &f.admin,
            &Category::AI,
            &soroban_sdk::String::from_str(&f.e, "ai"),
            &0,
            &0
        ),
        Error::Unauthorized
    );
}
