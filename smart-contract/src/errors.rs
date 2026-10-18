//! The frozen error table.
//!
//! # Two families of refusal, and they are not the same thing
//!
//! This enum is the **first** family: the call is refused, the transaction reverts, and nothing
//! is written. It is what a caller sees as `Error(Contract, #N)`.
//!
//! The second family is [`crate::types::ReasonCode`], and it is not an error at all. When
//! `authorize_spend` turns a spend down, the call **succeeds** — it writes a `SpendRecord` whose
//! `decision` is `Declined` and whose `reason_code` says why. That is deliberate, and it is what
//! makes a declined spend something a reviewer can open on an explorer: a refusal that reverted
//! would leave no transaction and no record for the evidence table to point at.
//!
//! So: a frozen card does **not** produce `Error::CardFrozen`. There is no such variant. It
//! produces a successful transaction carrying `ReasonCode::CardFrozen`. Conflating the two is
//! the single easiest mistake to make in this contract, and `docs/08-events-and-errors.md`
//! spells out which entry point speaks which language.
//!
//! # The numbers are part of the interface
//!
//! `demo.sh` greps for `Error(Contract, #N)` by exact number and fails the run on a mismatch,
//! the indexer branches on them, and the test suite asserts them. **Never renumber a variant.**
//! Add at the end.

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// `initialize` reached a contract that already has a `Config`. See `M1`.
    AlreadyInitialized = 1,
    /// An entry point ran before the contract was configured.
    ///
    /// **Unreachable, and kept anyway.** `__constructor` exists, so the host runs it on every
    /// deployment and refuses a deployment that omits its arguments — there is no route to a
    /// deployed-but-unconfigured contract. The guard stays because the alternative to returning
    /// this code is an `unwrap` on `Config`, and a defensive `Result` that never fires is
    /// cheaper than a panic that fires once.
    NotInitialized = 2,
    /// The caller does not hold the role this entry point requires, or did not sign.
    Unauthorized = 3,
    /// A non-positive amount. Zero is refused as well as negative: a zero-value spend record is
    /// noise in the ledger the reconciliation table has to read.
    InvalidAmount = 4,
    CardNotFound = 5,
    /// `allocate` asked for more than `unallocated`. The deck calls this `OVER_ALLOCATION`.
    OverAllocation = 6,
    /// `deallocate`, `transfer_between_cards` or `settle` asked a card for more than it holds.
    InsufficientCardBalance = 7,
    /// `settle` named a destination that is not on the whitelist.
    NotWhitelisted = 8,
    SpendNotFound = 9,
    /// `settle` named a spend that was declined. Only an approved spend can move money.
    SpendNotApproved = 10,
    /// `settle` asked for more than the spend it claims to settle.
    AmountExceedsSpend = 11,
    /// The spend has a settlement already. Invariant I3.
    AlreadySettled = 12,
    /// `revoke_role` would have removed the last admin, leaving the contract with no one able to
    /// allocate, create a card or manage the whitelist — unrecoverable, because there is no
    /// upgrade path and no super-user. See `M2`.
    LastAdmin = 13,
    /// A negative spending limit. Zero is legal and means "this card may not spend".
    InvalidLimit = 14,
    /// A limit window of zero seconds, which would make `spent_in_window` reset on every call
    /// and the limit meaningless.
    InvalidWindow = 15,
    /// `transfer_between_cards` was given the same card twice.
    SameCard = 16,
    /// `revoke_role` named an address that holds no role.
    RoleNotFound = 17,
    /// A card label that is empty or longer than 32 characters.
    InvalidLabel = 18,
    /// A merchant reference that is empty or longer than 64 characters.
    InvalidMerchantRef = 19,
    /// `add_whitelist` on an address already whitelisted, or `remove_whitelist` on one that is
    /// not. Refused rather than ignored, so a script cannot report success for a no-op.
    WhitelistUnchanged = 20,

    // ---- appended after the table was first frozen. Never renumber; only add. ----
    /// `get_settlement` was given an id no settlement has.
    ///
    /// Added after `get_settlement` was found returning `SpendNotFound` (9) for this — which is a
    /// claim about a different ledger, and would send somebody looking for a spend that exists
    /// perfectly well.
    SettlementNotFound = 21,
    /// A settlement destination that must never be one: the treasury contract itself.
    ///
    /// # Why this needs its own guard
    ///
    /// Settling to the contract's own address is a token transfer from the contract to the
    /// contract — a no-op. The books move anyway: the card is debited and `total_settled` rises,
    /// so that amount stops belonging to any card *and* never returns to `unallocated`. The
    /// tokens are still sitting in the contract, accounted to nobody, and no entry point can
    /// reach them again. Money is not stolen; it is stranded, permanently.
    ///
    /// The mistake it prevents is an ordinary one — a contract id and a settlement address are
    /// both Stellar strkeys and look alike in a config file. Refused at `add_whitelist`, so it
    /// fails when the mistake is made, and again at `settle`, so a whitelist written before this
    /// guard existed cannot spend through it.
    InvalidDestination = 22,
}
