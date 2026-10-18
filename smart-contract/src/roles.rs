//! Module M2 — who is allowed to do what.
//!
//! # There is no super-user
//!
//! `Admin` is the highest privilege that exists. Nobody can upgrade the contract, nobody can
//! change the token, and there is no recovery address. That makes one guard load-bearing:
//! [`revoke`] refuses to remove the last admin. A contract with no admin can never create a
//! card, allocate, or manage the whitelist again — the money is not stolen, it is stranded, and
//! there is nothing to do about it afterwards.
//!
//! The founder named at construction is seeded as an admin like any other. It holds no standing
//! privilege from being in `Config`; it can be revoked, provided somebody else holds `Admin`.

use soroban_sdk::{Address, Env, Map};

use crate::errors::Error;
use crate::storage;
use crate::types::Role;

/// `require_auth` on the caller **and** a role check.
///
/// Both, always, and in that order. `require_auth` proves the address signed; the role table
/// says whether that address may do this. Checking only the first authorises anyone who can
/// sign for themselves, which is everyone.
pub fn require(e: &Env, who: &Address, role: Role) -> Result<(), Error> {
    who.require_auth();
    match storage::roles(e).get(who.clone()) {
        Some(held) if held == role => Ok(()),
        // An admin is not implicitly an operator. The two roles guard different things — the
        // shape of the money versus its movement — and an implicit promotion would mean the
        // negative tests for one role pass for the wrong reason.
        _ => Err(Error::Unauthorized),
    }
}

/// `set_card_status` is the one entry point either role may call: an admin freezes a card as
/// policy, an operator freezes one in response to something happening now.
pub fn require_any(e: &Env, who: &Address) -> Result<Role, Error> {
    who.require_auth();
    storage::roles(e)
        .get(who.clone())
        .ok_or(Error::Unauthorized)
}

pub fn grant(e: &Env, who: &Address, role: Role) {
    let mut r = storage::roles(e);
    r.set(who.clone(), role);
    storage::set_roles(e, &r);
}

pub fn revoke(e: &Env, who: &Address) -> Result<(), Error> {
    let mut r = storage::roles(e);
    let held = r.get(who.clone()).ok_or(Error::RoleNotFound)?;
    if held == Role::Admin && count(&r, Role::Admin) == 1 {
        return Err(Error::LastAdmin);
    }
    r.remove(who.clone());
    storage::set_roles(e, &r);
    Ok(())
}

fn count(r: &Map<Address, Role>, role: Role) -> u32 {
    let mut n = 0;
    for (_, held) in r.iter() {
        if held == role {
            n += 1;
        }
    }
    n
}
