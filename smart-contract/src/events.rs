//! The event catalogue — module M13.
//!
//! # Events are infrastructure here, not decoration
//!
//! M13's definition of done is that the indexer can rebuild the **entire** contract state from
//! the event stream and nothing else. That is a stronger requirement than "emit something on
//! each write", and it drives one unusual choice: a mutation emits the **whole resulting
//! struct**, not a delta.
//!
//! Deltas are smaller and they are a trap. A `Card` reassembled from a starting balance and a
//! run of `+100 / −40 / +25` events is correct only if the consumer saw every one of them, in
//! order, exactly once. Miss a page of RPC results and the number is wrong with nothing to say
//! so. A stream of complete structs is idempotent and order-tolerant: the last event for a card
//! id **is** the card, whatever happened before it and however many times it is replayed.
//!
//! That property is what makes decision #08 safe. Spend and settlement records archive
//! eventually; the history that outlives them lives here, and `rebuild_test.rs` asserts the
//! reconstruction matches the contract's own view functions.
//!
//! # Typed events, not `publish((topics), data)`
//!
//! Each event is a `#[contractevent]` struct, so its shape lands in the contract's spec. The
//! indexer reads that spec rather than carrying a hand-written copy of the field order, which is
//! the sort of duplication that stays correct until the day somebody adds a field.
//!
//! # Topics are what `getEvents` can filter on
//!
//! First topic is the event name; the rest are the ids worth filtering on. An id that lives in
//! the data section instead means a consumer wanting one card's history has to fetch and discard
//! everything else to find it.

use soroban_sdk::{contractevent, Address, Env, Symbol};

use crate::types::{Card, Role, Settlement, SpendRecord, Treasury};

#[contractevent(topics = ["init"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Initialized {
    #[topic]
    pub founder: Address,
    pub token: Address,
    pub decimals: u32,
}

/// `role` carries the new role, or `None` when it was revoked. One event for both directions:
/// a consumer applies "this address now holds X", and a revoke is "X is nothing", which needs no
/// separate case.
#[contractevent(topics = ["role"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleSet {
    #[topic]
    pub who: Address,
    pub role: Option<Role>,
}

/// Carries the resulting `Treasury`, so a consumer never has to add deposits up itself.
#[contractevent(topics = ["deposit"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposited {
    #[topic]
    pub from: Address,
    pub amount: i128,
    pub treasury: Treasury,
}

/// Every card mutation — creation, allocation, rebalance, limit change, freeze, a rolled window,
/// a settlement debit — emits this with the card as it now stands.
///
/// `action` says which it was, for a human reading the stream and for an indexer that wants to
/// render a history rather than a state. The state is in `card` either way, so a consumer that
/// ignores `action` entirely still reconstructs correctly.
#[contractevent(topics = ["card"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CardChanged {
    #[topic]
    pub card_id: u32,
    #[topic]
    pub action: Symbol,
    pub card: Card,
    pub treasury: Treasury,
}

/// Approved and declined alike. A declined spend is a first-class record (decision #04) and it
/// gets a first-class event.
///
/// Also re-emitted by `settle`, carrying the record with `settled` now true — so the spend
/// ledger rebuilt from events ends in the same state as the one in storage.
#[contractevent(topics = ["spend"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendRecorded {
    #[topic]
    pub card_id: u32,
    #[topic]
    pub spend_id: u32,
    pub spend: SpendRecord,
}

#[contractevent(topics = ["whitelist"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhitelistChanged {
    #[topic]
    pub who: Address,
    pub allowed: bool,
}

/// The reconciliation event. `spend_id` is a topic because the question a reviewer asks is always
/// "what settled *this* spend".
#[contractevent(topics = ["settle"])]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settled {
    #[topic]
    pub spend_id: u32,
    #[topic]
    pub settlement_id: u32,
    pub settlement: Settlement,
    pub treasury: Treasury,
}

// ---------------------------------------------------------------------------
// Emitters
// ---------------------------------------------------------------------------
//
// Thin wrappers so an entry point reads as one line of intent rather than four of struct
// literal, and so the id fields — which are always copies of something already in the payload —
// are filled in from one place instead of at every call site.

pub fn initialized(e: &Env, founder: &Address, token: &Address, decimals: u32) {
    Initialized {
        founder: founder.clone(),
        token: token.clone(),
        decimals,
    }
    .publish(e);
}

pub fn role_set(e: &Env, who: &Address, role: Option<Role>) {
    RoleSet {
        who: who.clone(),
        role,
    }
    .publish(e);
}

pub fn deposited(e: &Env, from: &Address, amount: i128, treasury: &Treasury) {
    Deposited {
        from: from.clone(),
        amount,
        treasury: treasury.clone(),
    }
    .publish(e);
}

pub fn card_changed(e: &Env, action: Symbol, card: &Card, treasury: &Treasury) {
    CardChanged {
        card_id: card.id,
        action,
        card: card.clone(),
        treasury: treasury.clone(),
    }
    .publish(e);
}

pub fn spend_recorded(e: &Env, spend: &SpendRecord) {
    SpendRecorded {
        card_id: spend.card_id,
        spend_id: spend.id,
        spend: spend.clone(),
    }
    .publish(e);
}

pub fn whitelist_changed(e: &Env, who: &Address, allowed: bool) {
    WhitelistChanged {
        who: who.clone(),
        allowed,
    }
    .publish(e);
}

pub fn settled(e: &Env, settlement: &Settlement, treasury: &Treasury) {
    Settled {
        spend_id: settlement.spend_id,
        settlement_id: settlement.id,
        settlement: settlement.clone(),
        treasury: treasury.clone(),
    }
    .publish(e);
}
