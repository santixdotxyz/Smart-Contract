//! Modules M5–M8 — the card's own rules: its limit window, and what "remaining" means.
//!
//! # Balance and limit are two constraints, not one
//!
//! It is the deck's most repeated point and the one thing a reader is most likely to flatten.
//! `balance` is money assigned to the card. `limit` is a ceiling on spending per window. A card
//! with $61,000 of balance and $13,000 left of a $50,000 monthly limit can approve $12,000 and
//! decline $18,900, and neither number explains the other. `authorize_spend` consults them as
//! separate checks returning separate reason codes, because a console that shows one bar cannot
//! tell a user which wall they hit.

use soroban_sdk::{Env, String};

use crate::errors::Error;
use crate::types::Card;

/// Decision #02's default: 30 days. The SOW never says whether the limit is per-transaction,
/// daily or monthly, so the period is a per-card field and this is only what `create_card` uses
/// when the caller does not care.
pub const DEFAULT_WINDOW_SECS: u64 = 30 * 24 * 60 * 60;

/// Ten years, and the bound exists to stop `window_start + limit_window_secs` overflowing.
///
/// # This was a live bug, not a theoretical one
///
/// Before the bound, `check_window` refused only zero. `create_card(…, window_secs: u64::MAX)` —
/// or the same value through `set_limit` — was accepted, and from then on **every** call that
/// rolled the window trapped on `attempt to add with overflow`: `authorize_spend`, `set_limit`,
/// `get_card_live`, `get_remaining_limit`. The card could no longer spend and its limit could no
/// longer be corrected, because correcting it also rolls the window. One mistyped argument
/// bricked a card permanently, with no route back.
///
/// Ten years is far past any meaningful budgeting period and leaves `window_start` — a Unix
/// timestamp, so around 1.7 × 10⁹ — nowhere near the top of a `u64`.
pub const MAX_WINDOW_SECS: u64 = 10 * 365 * 24 * 60 * 60;

/// Advance the limit window if it has expired, and say whether anything moved.
///
/// # Lazy, and why that is the right shape
///
/// Nothing on Stellar wakes a contract up. A window that reset "on schedule" would need a keeper
/// transaction every period, per card — an operational dependency whose failure mode is a card
/// silently stuck at last month's spending. Instead every call that cares rolls the window first
/// and then evaluates. The window is therefore always correct at the moment it is read, and
/// correct without anyone maintaining it.
///
/// It advances by **whole windows**, not to `now`. A card untouched for three months resumes on
/// the window boundary it would have had if it had been used every month, which keeps the reset
/// instants stable and predictable rather than sliding forward each time somebody happens to
/// call.
/// # Arithmetic
///
/// The end of the window is computed with `checked_add`, and an overflow is treated as *"the
/// window has not expired"* rather than as a trap. [`MAX_WINDOW_SECS`] already makes that
/// unreachable through the entry points; this is the second line of defence, because the failure
/// it prevents is a card nobody can read, write, or repair.
///
/// The rest cannot overflow and does not need guarding: `whole × limit_window_secs ≤ elapsed` by
/// construction, so `window_start` only ever advances to a point at or before `now`.
pub fn roll_window(e: &Env, card: &mut Card) -> bool {
    let now = e.ledger().timestamp();
    if card.limit_window_secs == 0 {
        return false;
    }
    match card.window_start.checked_add(card.limit_window_secs) {
        Some(ends) if now >= ends => {}
        _ => return false,
    }
    let elapsed = now - card.window_start;
    let whole = elapsed / card.limit_window_secs;
    card.window_start += whole * card.limit_window_secs;
    card.spent_in_window = 0;
    true
}

/// What is left of the limit in the current window. Never negative: `spent_in_window` can only
/// grow through approvals, each of which was already checked against this number.
pub fn remaining_limit(card: &Card) -> i128 {
    let left = card.limit - card.spent_in_window;
    if left < 0 {
        0
    } else {
        left
    }
}

/// Bounded so a label cannot be used to bloat an entry — a card is read on every authorization,
/// and storage read cost is charged by size.
pub fn check_label(label: &String) -> Result<(), Error> {
    if label.is_empty() || label.len() > 32 {
        return Err(Error::InvalidLabel);
    }
    Ok(())
}

pub fn check_limit(limit: i128) -> Result<(), Error> {
    // Zero is legal and means "this card may not spend". It is the resting state of a card that
    // has been created but not yet authorised to do anything, and it is a meaningfully different
    // state from frozen: a zero-limit card declines with LIMIT_EXCEEDED, which says the card is
    // fine and the budget is not.
    if limit < 0 {
        return Err(Error::InvalidLimit);
    }
    Ok(())
}

/// Both ends of the range are refusals, and for different reasons.
///
/// Zero would reset the limit on every call, which makes the limit mean nothing. Anything past
/// [`MAX_WINDOW_SECS`] would overflow the window's end and leave the card unreadable — see the
/// note on that constant.
pub fn check_window(secs: u64) -> Result<(), Error> {
    if secs == 0 || secs > MAX_WINDOW_SECS {
        return Err(Error::InvalidWindow);
    }
    Ok(())
}
