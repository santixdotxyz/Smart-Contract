//! Module M13's definition of done: **the event stream alone reproduces the contract's state.**
//!
//! # Why this is the test that makes decision #08 safe
//!
//! Persistent entries on Soroban archive. Cards, spend records and settlements all have a TTL,
//! and the contract bumps them on every touch — but an entry nobody touches for long enough goes
//! away regardless, and the deck asks (decision #08) whether the long history should be kept
//! alive on chain or reconstructed off it.
//!
//! The answer is "reconstructed", and this is the evidence for it. The fold below replays
//! nothing but events, rebuilds every card, spend and settlement, and compares the result
//! against what the contract's own view functions return. If that holds, `backend/indexer` can
//! rebuild the same history from RPC and archival stops being a data-loss problem.
//!
//! # The property being relied on
//!
//! Events carry **whole structs, not deltas** (see [`crate::events`]), so the reconstruction is
//! a fold that keeps the last event per id: idempotent, and tolerant of a page being replayed.
//! [`replaying_the_stream_twice_changes_nothing`] is the test for that half.
//!
//! # Absorbing after every call rather than reading a full log at the end
//!
//! `Env::events().all()` returns what the **most recent** invocation published, not the whole
//! history — so a fold written at the end of a scenario would silently see one transaction's
//! worth of events and pass on a subset. Absorbing after each call is also the closer model of
//! what an indexer does: it consumes pages as they arrive and never has the whole stream in
//! hand at once.

use soroban_sdk::testutils::Events as _;
use soroban_sdk::xdr::{ContractEventBody, ScVal};
use soroban_sdk::{symbol_short, Address, Map, Symbol, TryFromVal, Val, Vec};

use super::harness::{DAY, F, USD};
use crate::types::{Card, Category, Settlement, SpendRecord, Treasury};

/// The state a consumer can rebuild knowing only the event stream.
struct Rebuilt {
    cards: Map<u32, Card>,
    spends: Map<u32, SpendRecord>,
    settlements: Map<u32, Settlement>,
    treasury: Treasury,
    whitelist: Map<Address, bool>,
}

impl Rebuilt {
    fn new(f: &F) -> Self {
        Rebuilt {
            cards: Map::new(&f.e),
            spends: Map::new(&f.e),
            settlements: Map::new(&f.e),
            treasury: Treasury {
                total_deposited: 0,
                total_allocated: 0,
                total_settled: 0,
            },
            whitelist: Map::new(&f.e),
        }
    }

    /// Fold in whatever the last invocation published.
    fn absorb(&mut self, f: &F) {
        let e = &f.e;
        // Filtering by contract is not decoration: an indexer pointed at a real network sees
        // every contract in the ledger and has to do exactly this.
        for ev in
            f.e.events()
                .all()
                .filter_by_contract(&f.contract_id)
                .events()
        {
            let ContractEventBody::V0(body) = &ev.body;
            let name: Symbol = decode(e, &body.topics[0]);
            let data: Val = Val::try_from_val(e, &body.data).unwrap();
            // Default `#[contractevent]` data format: a map keyed by the field name.
            let d: Map<Symbol, Val> = Map::try_from_val(e, &data).unwrap();

            if name == symbol_short!("card") {
                let card: Card = field(e, &d, symbol_short!("card"));
                self.treasury = field(e, &d, symbol_short!("treasury"));
                self.cards.set(card.id, card);
            } else if name == symbol_short!("spend") {
                let s: SpendRecord = field(e, &d, symbol_short!("spend"));
                self.spends.set(s.id, s);
            } else if name == symbol_short!("settle") {
                let s: Settlement = field(e, &d, Symbol::new(e, "settlement"));
                self.treasury = field(e, &d, symbol_short!("treasury"));
                self.settlements.set(s.id, s);
            } else if name == symbol_short!("deposit") {
                self.treasury = field(e, &d, symbol_short!("treasury"));
            } else if name == Symbol::new(e, "whitelist") {
                // `who` is a **topic**, so it is not in the data map — that is the whole point of
                // marking it as one, since an indexer wants to filter on it without decoding
                // every event body.
                let who: Address = decode(e, &body.topics[1]);
                let allowed: bool = field(e, &d, symbol_short!("allowed"));
                self.whitelist.set(who, allowed);
            }
        }
    }
}

fn decode<T: TryFromVal<soroban_sdk::Env, Val>>(e: &soroban_sdk::Env, v: &ScVal) -> T {
    let val = Val::try_from_val(e, v).unwrap();
    T::try_from_val(e, &val).unwrap_or_else(|_| panic!("event topic did not decode"))
}

fn field<T: TryFromVal<soroban_sdk::Env, Val>>(
    e: &soroban_sdk::Env,
    d: &Map<Symbol, Val>,
    key: Symbol,
) -> T {
    let raw = d
        .get(key)
        .expect("event is missing a field the catalogue promises");
    T::try_from_val(e, &raw)
        .unwrap_or_else(|_| panic!("event field did not decode — catalogue and consumer disagree"))
}

/// Drive a scenario that touches every mutation, folding events as they are published, then
/// compare the reconstruction against the contract.
#[test]
fn the_event_stream_alone_reproduces_the_contract_state() {
    let f = F::new();
    let mut r = Rebuilt::new(&f);
    // A closure would borrow `f` for the rest of the function, so the absorb is written out.
    macro_rules! step {
        ($($call:tt)*) => {{ let out = { $($call)* }; r.absorb(&f); out }};
    }

    step!(f.deposit(250_000 * USD));
    step!(f.deposit(50_000 * USD));
    step!(f.whitelist(&f.settlement));
    step!(f.whitelist(&f.rogue));
    step!(f.c.remove_whitelist(&f.admin, &f.rogue));

    let saas = step!(f.card(Category::SaaS, "saas", 50_000 * USD));
    let ai = step!(f.card(Category::AI, "ai-tools", 50_000 * USD));
    let ads = step!(f.card(Category::Advertising, "ads", 50_000 * USD));
    let cloud = step!(f.card(Category::Cloud, "cloud", 25_000 * USD));

    step!(f.allocate(saas, 42_300 * USD));
    step!(f.allocate(ai, 61_000 * USD));
    step!(f.allocate(ads, 54_200 * USD));
    step!(f.allocate(cloud, 25_000 * USD));

    step!(f.c.deallocate(&f.admin, &ads, &(4_200 * USD)));
    step!(f
        .c
        .transfer_between_cards(&f.admin, &ai, &saas, &(1_000 * USD)));
    step!(f.c.set_limit(&f.admin, &ai, &(40_000 * USD), &0));
    step!(f.freeze(cloud));

    let ok = step!(f.spend(saas, 2_400 * USD, "aws"));
    step!(f.spend(cloud, 100 * USD, "gcp-while-frozen"));
    step!(f.spend(ai, 90_000 * USD, "over-limit"));
    let second = step!(f.spend(ads, 12_000 * USD, "google-ads"));

    // A window roll, so the rebuilt card carries a `spent_in_window` that only the event emitted
    // by the roll itself could explain.
    f.advance(45 * DAY);
    step!(f.spend(saas, 1, "after-the-window"));

    step!(f.settle(ok.id, 2_400 * USD));
    step!(f.settle(second.id, 12_000 * USD));
    f.check_books();

    // ---- and now, knowing only what the events said ----

    assert_eq!(r.treasury, f.c.get_treasury(), "treasury");

    let live: Vec<Card> = f.c.list_cards();
    assert_eq!(r.cards.len(), live.len(), "card count");
    for c in live.iter() {
        assert_eq!(
            r.cards.get(c.id).expect("card missing from the rebuild"),
            c,
            "card {}",
            c.id
        );
    }

    let spends: Vec<SpendRecord> = f.c.list_spends(&0);
    assert_eq!(r.spends.len(), spends.len(), "spend count");
    for s in spends.iter() {
        // Includes `settled`, which is only right because `settle` re-emits the record. A
        // consumer that saw only the authorization event would show two paid invoices as still
        // outstanding — the exact error the reconciliation table exists to prevent.
        assert_eq!(
            r.spends.get(s.id).expect("spend missing from the rebuild"),
            s,
            "spend {}",
            s.id
        );
    }

    let settlements: Vec<Settlement> = f.c.list_settlements();
    assert_eq!(r.settlements.len(), settlements.len(), "settlement count");
    for s in settlements.iter() {
        assert_eq!(r.settlements.get(s.id).unwrap(), s, "settlement {}", s.id);
    }

    // The whitelist too. An address added and then removed must rebuild as removed — the case a
    // consumer that only handled additions would get wrong, and the one that decides whether a
    // settlement destination looks authorised.
    assert_eq!(r.whitelist.get(f.settlement.clone()), Some(true));
    assert_eq!(r.whitelist.get(f.rogue.clone()), Some(false));
    assert!(f.c.is_whitelisted(&f.settlement));
    assert!(!f.c.is_whitelisted(&f.rogue));
}

/// The fold is idempotent: absorbing the same events again changes nothing.
///
/// This is what makes an at-least-once RPC feed safe. An indexer that re-reads a page it has
/// already seen — after a restart, or because `getEvents` paged oddly — must not double-count,
/// and with whole structs rather than deltas it cannot.
#[test]
fn replaying_the_stream_twice_changes_nothing() {
    let f = F::new();
    f.deposit(1_000 * USD);
    f.whitelist(&f.settlement);
    let card = f.card(Category::SaaS, "saas", 1_000 * USD);
    f.allocate(card, 500 * USD);
    let r = f.spend(card, 100 * USD, "aws");
    f.settle(r.id, 100 * USD);

    let mut once = Rebuilt::new(&f);
    once.absorb(&f);
    let mut twice = Rebuilt::new(&f);
    twice.absorb(&f);
    twice.absorb(&f);
    twice.absorb(&f);

    // Only the settle transaction's events are in hand, which is enough for the property under
    // test: three absorbs of the same page produce what one did.
    assert_eq!(once.treasury, twice.treasury);
    assert_eq!(once.cards.get(card), twice.cards.get(card));
    assert_eq!(once.spends.get(r.id), twice.spends.get(r.id));
    assert_eq!(once.settlements.len(), twice.settlements.len());
    assert_eq!(once.settlements.len(), 1);
    f.check_books();
}
