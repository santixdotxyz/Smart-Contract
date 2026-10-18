//! Keys, typed accessors, and the TTL discipline.
//!
//! # Why every read goes through a function here
//!
//! `e.storage().persistent().get(&DataKey::Card(id))` written out at each call site is four
//! chances to reach for the wrong durability. Instance and persistent entries archive on
//! different schedules and an archived card is a card whose money cannot be moved — a failure
//! that needs no bug and no attacker, just eight quiet months. Routing every access through a
//! named function means the durability of a thing is decided once, here.
//!
//! # Decision #08: both halves of the TTL answer
//!
//! The deck offers a choice — bump TTL on every write, or let historical records live in events
//! and be reconstructed off-chain. The resolution is **both**, because they solve different
//! problems:
//!
//! - **Bumping** keeps the entries an entry point needs to *act* on alive: config, roles,
//!   treasury, the whitelist, and the cards. Losing one of those breaks the contract.
//! - **Events** are what the long history rests on. Spend and settlement records are bumped too,
//!   but the guarantee that history survives comes from `backend/indexer`, which rebuilds every
//!   record from the event stream alone. That property is asserted by `rebuild_test.rs`, and it
//!   is also module M13's definition of done.
//!
//! The bump is **bounded**: a call extends the keys it actually touched, never every card in the
//! contract. An unbounded bump grows with the number of cards until an entry point no longer
//! fits in a transaction.

use soroban_sdk::{contracttype, Address, Env, Map, Vec};

use crate::types::{Card, Config, Role, Settlement, SpendRecord, Treasury};

/// Roughly 17,280 ledgers a day at five seconds a ledger.
const DAY: u32 = 17_280;
/// Extend when fewer than 30 days remain …
const BUMP_THRESHOLD: u32 = 30 * DAY;
/// … back out to 90. Well inside the 180-day network maximum, so the extension is never
/// silently clamped to something shorter than the code claims.
const BUMP_AMOUNT: u32 = 90 * DAY;

#[contracttype]
pub enum DataKey {
    Config,
    Treasury,
    /// `Map<Address, Role>`. Small and read on nearly every call, so instance storage: it rides
    /// along with the contract instance and needs no separate TTL management.
    Roles,
    /// `Map<Address, ()>`. A set; the value carries nothing. Membership is the whole fact.
    Whitelist,
    /// Highest card id issued. Ids start at 1, so `0` reliably means "no cards".
    CardSeq,
    SpendSeq,
    SettlementSeq,
    Card(u32),
    Spend(u32),
    Settlement(u32),
    /// `spend_id → settlement_id`. The index behind I3 — the existence of this key is what makes
    /// a second settle refusable without scanning the settlement ledger.
    SpendSettlement(u32),
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

pub fn has_config(e: &Env) -> bool {
    e.storage().instance().has(&DataKey::Config)
}

pub fn config(e: &Env) -> Option<Config> {
    e.storage().instance().get(&DataKey::Config)
}

pub fn set_config(e: &Env, c: &Config) {
    e.storage().instance().set(&DataKey::Config, c);
}

pub fn treasury(e: &Env) -> Treasury {
    e.storage()
        .instance()
        .get(&DataKey::Treasury)
        .unwrap_or(Treasury {
            total_deposited: 0,
            total_allocated: 0,
            total_settled: 0,
        })
}

pub fn set_treasury(e: &Env, t: &Treasury) {
    e.storage().instance().set(&DataKey::Treasury, t);
}

pub fn roles(e: &Env) -> Map<Address, Role> {
    e.storage()
        .instance()
        .get(&DataKey::Roles)
        .unwrap_or_else(|| Map::new(e))
}

pub fn set_roles(e: &Env, r: &Map<Address, Role>) {
    e.storage().instance().set(&DataKey::Roles, r);
}

pub fn whitelist(e: &Env) -> Map<Address, ()> {
    e.storage()
        .instance()
        .get(&DataKey::Whitelist)
        .unwrap_or_else(|| Map::new(e))
}

pub fn set_whitelist(e: &Env, w: &Map<Address, ()>) {
    e.storage().instance().set(&DataKey::Whitelist, w);
}

// ---------------------------------------------------------------------------
// Sequences
// ---------------------------------------------------------------------------

/// Hand out the next id and store it. Ids are monotonic and never reused — a reused card id
/// would make the spend history of two different cards indistinguishable in the event stream.
fn next(e: &Env, key: DataKey) -> u32 {
    let n: u32 = e.storage().instance().get(&key).unwrap_or(0) + 1;
    e.storage().instance().set(&key, &n);
    n
}

pub fn next_card_id(e: &Env) -> u32 {
    next(e, DataKey::CardSeq)
}

pub fn next_spend_id(e: &Env) -> u32 {
    next(e, DataKey::SpendSeq)
}

pub fn next_settlement_id(e: &Env) -> u32 {
    next(e, DataKey::SettlementSeq)
}

pub fn card_count(e: &Env) -> u32 {
    e.storage().instance().get(&DataKey::CardSeq).unwrap_or(0)
}

pub fn spend_count(e: &Env) -> u32 {
    e.storage().instance().get(&DataKey::SpendSeq).unwrap_or(0)
}

pub fn settlement_count(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&DataKey::SettlementSeq)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Persistent
// ---------------------------------------------------------------------------

pub fn card(e: &Env, id: u32) -> Option<Card> {
    e.storage().persistent().get(&DataKey::Card(id))
}

pub fn set_card(e: &Env, c: &Card) {
    let key = DataKey::Card(c.id);
    e.storage().persistent().set(&key, c);
    bump(e, &key);
}

pub fn spend(e: &Env, id: u32) -> Option<SpendRecord> {
    e.storage().persistent().get(&DataKey::Spend(id))
}

pub fn set_spend(e: &Env, s: &SpendRecord) {
    let key = DataKey::Spend(s.id);
    e.storage().persistent().set(&key, s);
    bump(e, &key);
}

pub fn settlement(e: &Env, id: u32) -> Option<Settlement> {
    e.storage().persistent().get(&DataKey::Settlement(id))
}

pub fn set_settlement(e: &Env, s: &Settlement) {
    let key = DataKey::Settlement(s.id);
    e.storage().persistent().set(&key, s);
    bump(e, &key);
    let link = DataKey::SpendSettlement(s.spend_id);
    e.storage().persistent().set(&link, &s.id);
    bump(e, &link);
}

/// The settlement that closed this spend, if any. Backs invariant I3.
pub fn settlement_of_spend(e: &Env, spend_id: u32) -> Option<u32> {
    e.storage()
        .persistent()
        .get(&DataKey::SpendSettlement(spend_id))
}

// ---------------------------------------------------------------------------
// Enumeration
// ---------------------------------------------------------------------------
//
// Ids are dense from 1 to the sequence value, so listing is a range walk rather than a stored
// index. `get` on a missing key returns `None`, which is what an archived entry looks like too —
// the walk skips both rather than failing, so one archived card does not make `list_cards`
// unusable for the rest.

pub fn list_cards(e: &Env) -> Vec<Card> {
    let mut out = Vec::new(e);
    for id in 1..=card_count(e) {
        if let Some(c) = card(e, id) {
            out.push_back(c);
        }
    }
    out
}

pub fn list_spends(e: &Env, card_id: Option<u32>) -> Vec<SpendRecord> {
    let mut out = Vec::new(e);
    for id in 1..=spend_count(e) {
        if let Some(s) = spend(e, id) {
            match card_id {
                Some(want) if s.card_id != want => continue,
                _ => out.push_back(s),
            }
        }
    }
    out
}

pub fn list_settlements(e: &Env) -> Vec<Settlement> {
    let mut out = Vec::new(e);
    for id in 1..=settlement_count(e) {
        if let Some(s) = settlement(e, id) {
            out.push_back(s);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// TTL
// ---------------------------------------------------------------------------

fn bump(e: &Env, key: &DataKey) {
    e.storage()
        .persistent()
        .extend_ttl(key, BUMP_THRESHOLD, BUMP_AMOUNT);
}

/// Step 10 of the write discipline. Always last, always bounded.
pub fn bump_instance(e: &Env) {
    e.storage()
        .instance()
        .extend_ttl(BUMP_THRESHOLD, BUMP_AMOUNT);
}
