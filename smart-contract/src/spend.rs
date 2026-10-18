//! Module M9 — the four checks, in the order that is frozen.
//!
//! # This function does not move money
//!
//! `authorize_spend` decides and records. Nothing transfers, and no card balance changes. The
//! card is debited later, by `settle`, when the payment has actually been made off-chain and
//! there is a settlement address to pay. Between the two the money is still on the card and
//! still counted by the treasury — which is why `spent_in_window` rises on approval while
//! `balance` does not, and why an approved-but-unsettled spend does not reduce what the next
//! authorization sees as available balance.
//!
//! That is the race window the deck calls decision #01, stated in code: two approvals against
//! one card can both pass check 3 before either settles. The resolution is that the contract is
//! the final arbiter — `settle` re-checks the balance and refuses the second one — and the
//! gateway owns the orphan approval. See `docs/07-invariants-and-security.md`.
//!
//! # Why the order is frozen
//!
//! Two conditions often fail together: a frozen card is usually also over its limit, because
//! that is why somebody froze it. Which reason comes back is decided entirely by the order the
//! checks run in, and the off-chain gateway applies the same four rules from its own cached copy
//! of the state. If the two disagree about the order, they give different reasons for the same
//! declined transaction, and the reconciliation table stops being an explanation of anything.
//!
//! Status, then limit, then card balance, then treasury backing. Cheapest and most categorical
//! first — a frozen card is a policy decision and no arithmetic should override it.

use crate::invariant;
use crate::types::{Card, CardStatus, ReasonCode, Treasury};

/// Evaluate the four rules and return the first failure, or `None` for approved.
///
/// Pure: it takes the state and returns a verdict, touching neither storage nor the ledger. That
/// is what lets `spend_tests.rs` enumerate the boundary cases directly and what would let the
/// gateway link against the same logic rather than reimplementing it.
///
/// `token_balance` is the contract's **real** balance, read from the token contract by the
/// caller. Check 4 deliberately does not consult our own books: the books cannot see a clawback,
/// and the books are exactly what would be wrong if they could.
pub fn evaluate(
    card: &Card,
    amount: i128,
    treasury: &Treasury,
    token_balance: i128,
) -> Option<ReasonCode> {
    // 1 — status.
    if card.status == CardStatus::Frozen {
        return Some(ReasonCode::CardFrozen);
    }

    // 2 — remaining limit for this window. The window has already been rolled by the caller;
    // evaluating against a stale window would decline spending that a fresh period allows.
    if amount > crate::cards::remaining_limit(card) {
        return Some(ReasonCode::LimitExceeded);
    }

    // 3 — the card's own money.
    if amount > card.balance {
        return Some(ReasonCode::InsufficientCardBalance);
    }

    // 4 — can the contract actually pay? Invariant I2. Unreachable while the token behaves;
    // reachable after an issuer clawback, which is the case this check exists for.
    if invariant::obligation(treasury) > token_balance || amount > token_balance {
        return Some(ReasonCode::TreasuryInsolvent);
    }

    None
}
