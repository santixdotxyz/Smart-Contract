//! The properties that must hold after every write, and where each one is checked.
//!
//! # Three invariants, and they are not all checked in the same place
//!
//! **I1 — accounting identity.** `unallocated ≥ 0` and `Σ card.balance ≥ 0`, expressed through
//! the running totals:
//!
//! ```text
//! unallocated    = total_deposited − total_allocated  ≥ 0
//! Σ card.balance = total_allocated − total_settled    ≥ 0
//! ```
//!
//! Checked **inside the contract**, after every write, by [`assert_books`]. It is cheap because
//! it reads three numbers rather than every card — and that is the whole reason the totals exist.
//!
//! **I2 — backing.** `total_deposited − total_settled ≤ token.balance(contract)`. The contract
//! holds at least what it owes.
//!
//! *Not* asserted after writes, and the difference matters. I1 is about our own arithmetic and
//! can only break if the code is wrong. I2 is about the outside world: on Stellar an issuer with
//! clawback enabled can remove tokens from any holder, this contract included, and no bug on our
//! side is involved. Panicking would freeze a treasury that is merely short. So I2 is *consulted*
//! — it is check 4 of `authorize_spend`, and failing it produces `ReasonCode::TreasuryInsolvent`
//! on a recorded, declined spend rather than a revert.
//!
//! **I3 — no double settle.** Each `spend_id` appears at most once in the settlement ledger.
//! Enforced structurally by the `SpendSettlement(spend_id)` key in [`crate::storage`]: `settle`
//! refuses with `AlreadySettled` if it exists. There is no scan and no window in which two
//! settlements for one spend could both pass their checks.
//!
//! # What the contract cannot check, and the tests must
//!
//! The identity in I1 is self-consistent by construction: if `allocate` forgot to move
//! `total_allocated`, the totals would still agree with each other. Only summing the actual
//! cards catches that, and only comparing against the token's real balance catches a missing
//! transfer. Both live in the test harness's `check_books`, which every scenario ends with — and
//! that is where a missing transfer hides while the books look perfect.

use crate::types::Treasury;

/// The cheap half. Called at step 7 of every write, before the event and before any transfer.
///
/// Panics rather than returning an error, on purpose. An `Error` is something a caller did; this
/// is something the contract did, and there is no caller behaviour that should produce it. A
/// numbered error code here would invite the gateway to branch on a bug.
pub fn assert_books(t: &Treasury) {
    if t.total_deposited < 0 || t.total_allocated < 0 || t.total_settled < 0 {
        panic!("I1: a running total went negative");
    }
    if t.total_allocated > t.total_deposited {
        panic!("I1: allocated exceeds deposited — unallocated would be negative");
    }
    if t.total_settled > t.total_allocated {
        panic!("I1: settled exceeds allocated — sum of card balances would be negative");
    }
}

/// `total_deposited − total_allocated`. The only source `allocate` may draw on.
pub fn unallocated(t: &Treasury) -> i128 {
    t.total_deposited - t.total_allocated
}

/// `total_allocated − total_settled`. What the sum of every card balance must equal; the test
/// harness checks it against the actual sum.
pub fn allocated_outstanding(t: &Treasury) -> i128 {
    t.total_allocated - t.total_settled
}

/// What the contract owes: everything deposited that has not been paid out. I2 says the token
/// balance is at least this.
pub fn obligation(t: &Treasury) -> i128 {
    t.total_deposited - t.total_settled
}
