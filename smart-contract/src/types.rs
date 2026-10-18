//! The state layout. Frozen before the first entry point was written, because changing a
//! `#[contracttype]` after deployment is a migration, and Phase 1 has no upgrade path.

use soroban_sdk::{contracttype, Address, String};

/// Who may do what.
///
/// Two roles, not three. The deck's actor list has an "auth gateway" as well, but the gateway
/// never holds a distinct privilege — it signs as the operator. Giving it a role of its own
/// would be a name in storage that gates nothing.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Role {
    /// Owns the money's shape: creates cards, allocates, sets limits, manages the whitelist and
    /// the role table itself.
    Admin,
    /// Owns the money's movement: authorises spends and settles them. A hot key by necessity —
    /// see `docs/07-invariants-and-security.md`.
    Operator,
}

/// The cost categories, as an **enum and not free text**.
///
/// The deck is emphatic about this (slide 12) and the reason is downstream: a report that groups
/// spending by category can only be clean if the category is drawn from a closed set. `Other`
/// exists so that a real expense never has to be mislabelled into one of the four named ones.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Category {
    SaaS,
    AI,
    Advertising,
    Cloud,
    Other,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CardStatus {
    Active,
    Frozen,
}

/// The outcome recorded by `authorize_spend`.
///
/// An enum rather than a `bool` on purpose. A real card network has authorization, capture, void,
/// refund and chargeback; Phase 1 models the first two outcomes only, and the deck asks
/// explicitly (slide 15) that we not close the door on the rest. Adding `Captured` to an enum is
/// a code change; adding it to a `bool` is a storage migration.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Approved,
    Declined,
}

/// Why a spend was turned down — **data, not an error**.
///
/// A declined `authorize_spend` returns `Ok`. The transaction succeeds, a `SpendRecord` is
/// written, and this is the field that says why. See the note at the top of [`crate::errors`].
///
/// # The order of these variants is the order of the checks
///
/// `authorize_spend` evaluates status, then limit, then card balance, then treasury backing, and
/// returns the **first** thing that fails. That order is frozen: it decides which code comes
/// back when two conditions fail at once, and the off-chain gateway has to reproduce the same
/// answer or the two systems will give different reasons for the same declined transaction.
///
/// # No integer discriminants, deliberately
///
/// Giving these variants explicit numbers would make the SDK encode them as a `u32`, and a
/// reviewer opening a declined transaction would see `reason_code: 2` where the requirement is
/// that they can read *why* the spend was refused. As a plain enum it encodes as its own name,
/// so the ledger says `LimitExceeded` — which is the deck's `LIMIT_EXCEEDED`, in the vocabulary
/// the SOW's evidence table uses.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    /// Approved. Carried on the record so every spend has a reason field with a defined value.
    None,
    /// Check 1. The card is frozen; nothing else was consulted.
    CardFrozen,
    /// Check 2. The amount exceeds what is left of the card's limit in the current window.
    LimitExceeded,
    /// Check 3. The card's own balance is short.
    InsufficientCardBalance,
    /// Check 4. The contract does not actually hold enough of the token to pay.
    ///
    /// Unreachable through the contract's own accounting — the identity in
    /// [`crate::invariant`] guarantees the treasury covers every card. It becomes reachable when
    /// tokens leave without the contract's knowledge, which on Stellar means an issuer clawback.
    /// That is why the check reads the token's real balance instead of the books.
    TreasuryInsolvent,
}

/// Set once, at construction, and never written again.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// The address named at construction. Kept for the record — authority comes from the role
    /// table, which this address is seeded into and can later be revoked from, provided somebody
    /// else holds `Admin` by then.
    pub founder: Address,
    /// The one token this treasury accepts. `deposit` refuses anything else by construction —
    /// there is no token parameter to get wrong.
    pub token: Address,
    /// Read from the token contract at construction, never taken from the deployer. USDC on
    /// Stellar is 7; the field exists so an explorer and the indexer can render amounts without
    /// a second cross-contract call.
    pub decimals: u32,
}

/// Three running totals, and every number the treasury reports is derived from them.
///
/// ```text
/// unallocated       = total_deposited − total_allocated
/// Σ card.balance    = total_allocated − total_settled
/// ```
///
/// Both identities are asserted after every write. Keeping the totals rather than summing the
/// cards is not an optimisation — summing would mean loading every card on every call, and the
/// cost of that grows with the number of cards until an entry point stops fitting in a
/// transaction.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Treasury {
    /// Everything ever deposited. Never decreases.
    pub total_deposited: i128,
    /// Everything currently assigned to a card, **including** amounts already settled out of it.
    /// `allocate` raises it, `deallocate` lowers it, `settle` leaves it alone — which is what
    /// makes the second identity above hold.
    pub total_allocated: i128,
    /// Everything ever paid out to a settlement address. Never decreases.
    pub total_settled: i128,
}

/// One spending category, with a balance and a limit that are **two independent constraints**.
///
/// The deck says it twice and it is worth saying a third time: `balance` is money that is
/// actually here and assigned to this card. `limit` is a ceiling on how much may be spent per
/// window regardless of how much money is here. A card can be rich and out of limit, or well
/// under its limit and broke. Merging them is the design mistake that is hardest to undo later.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Card {
    pub id: u32,
    pub category: Category,
    pub label: String,
    /// Money assigned to this card. Raised by `allocate`, lowered by `deallocate` and `settle`.
    pub balance: i128,
    /// Ceiling per window. `0` is legal and means the card may not spend.
    pub limit: i128,
    /// Length of the limit window in seconds. Decision #02: the SOW does not say whether the
    /// limit is per-transaction, daily or monthly, so it is a per-card parameter with a 30-day
    /// default and the answer is recorded on the card rather than assumed.
    pub limit_window_secs: u64,
    /// When the current window opened. Rolled **lazily**: any call that finds the window expired
    /// advances it before evaluating anything. No keeper, no cron, no scheduled transaction.
    pub window_start: u64,
    /// Approved spending inside the current window. Reset to zero when the window rolls.
    pub spent_in_window: i128,
    pub status: CardStatus,
}

/// The outcome of one authorization, approved or declined, written to persistent storage.
///
/// Decision #04: **declines are recorded, not just emitted.** It costs storage and the deck
/// floats emitting an event instead. The evidence requirement decides it — SOW §6 wants a
/// reviewer with minimal technical expertise to open a declined transaction and see the reason,
/// and the deck's own Spend Simulator wireframe shows `DECLINED · LIMIT_EXCEEDED` next to a
/// transaction hash. An event-only decline gives the reviewer nothing to query.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpendRecord {
    pub id: u32,
    pub card_id: u32,
    /// The amount requested. On a declined record this is what was **asked for**, not what moved
    /// — nothing moved.
    pub amount: i128,
    /// Free text from the gateway identifying the merchant or subscription. Bounded at 64 bytes.
    pub merchant_ref: String,
    pub decision: Decision,
    pub reason_code: ReasonCode,
    /// Set by `settle`. Enforces I3: a spend settles at most once.
    pub settled: bool,
    /// The ledger this record was written in. UX principle 05 in the deck: "approved" and
    /// "written on chain" are different moments, at least one ledger apart, and a console that
    /// shows them as one is lying about the race window.
    pub ledger_seq: u32,
    /// Wall-clock of the ledger, so the indexer can order records without a second RPC call.
    pub timestamp: u64,
}

/// The reconciliation record: the row that turns a transaction hash into evidence by naming the
/// spend that caused it.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    pub id: u32,
    /// The link. Without it a settlement is a transfer nobody can account for.
    pub spend_id: u32,
    /// Denormalised from the spend so the reconciliation table can be built from settlements
    /// alone, which is what the indexer's `report` does.
    pub card_id: u32,
    pub amount: i128,
    pub to: Address,
    pub ledger_seq: u32,
    pub timestamp: u64,
}
