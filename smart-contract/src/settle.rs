//! Modules M10–M12 — the only place money leaves the contract.
//!
//! # Five guards, not three
//!
//! The deck names three (operator, whitelisted address, amount within the spend). Two more are
//! required for the guards to mean anything, and both come from the fact that `authorize_spend`
//! records declines as first-class rows:
//!
//! - the spend must be **approved**. Without this check, a declined spend — a row that exists
//!   precisely because the contract refused it — would be settleable, and the refusal would have
//!   been decorative.
//! - the spend must not be **settled already**. Invariant I3.
//!
//! Caller identity is checked by [`crate::roles::require`] at the entry point; the rest are here.
//!
//! # Debit the originating card, never the unallocated pool
//!
//! The SOW is explicit and it is the part most likely to be got wrong under time pressure,
//! because taking it from unallocated is simpler and the treasury total comes out the same. It
//! is not the same. The card's balance is the record of what that category has left to spend,
//! and settling elsewhere would leave every category report wrong while the top-line number
//! looked right. `settle` therefore also re-checks the card balance — the approval that
//! authorised this spend may be several ledgers old, and another settlement may have drained the
//! card since.

use crate::errors::Error;
use crate::types::{Card, Decision, SpendRecord};

/// Every guard that can be decided from state already loaded. Ordered cheapest first, and
/// ordered so the message a caller gets names the most specific thing wrong.
pub fn check(
    spend: &SpendRecord,
    card: &Card,
    amount: i128,
    already_settled: Option<u32>,
    whitelisted: bool,
) -> Result<(), Error> {
    if amount <= 0 {
        return Err(Error::InvalidAmount);
    }
    if !whitelisted {
        return Err(Error::NotWhitelisted);
    }
    if spend.decision != Decision::Approved {
        return Err(Error::SpendNotApproved);
    }
    // Two ways to be settled, and both are checked. `spend.settled` is the flag on the record;
    // the index is the structural fact. They cannot disagree unless something is very wrong, and
    // checking both costs one storage read that has already happened.
    if spend.settled || already_settled.is_some() {
        return Err(Error::AlreadySettled);
    }
    // Settling for LESS than the spend is allowed. A real settlement often is less — a partial
    // capture, a discount applied after authorization — and refusing it would force the operator
    // to choose between over-paying and leaving the spend permanently open.
    if amount > spend.amount {
        return Err(Error::AmountExceedsSpend);
    }
    if amount > card.balance {
        return Err(Error::InsufficientCardBalance);
    }
    Ok(())
}
