#![no_std]
//! # Saintix — an on-chain treasury with category virtual cards
//!
//! One contract per organisation. It holds that organisation's USDC, splits it into
//! category-based virtual cards each with its own balance, limit and status, records every spend
//! authorization decision, and settles approved spends to whitelisted addresses with a
//! reconciliation record linking each payment back to the spend that caused it.
//!
//! # The contract is a rule book and a ledger, not a payment gateway
//!
//! This is the architectural claim everything else follows from. A card network wants an
//! authorization answer in about two seconds; a Stellar ledger closes in about five. The
//! contract therefore cannot sit in the path of a card swipe, and it does not try to.
//!
//! - The **off-chain gateway** reads this contract's state over RPC, applies the same four rules,
//!   and answers the network in milliseconds.
//! - The contract is where those rules are **defined** and where the decision is **recorded**,
//!   at least one ledger later.
//!
//! The consequence is a race window, and it is accepted rather than engineered away — see
//! decision #01 below.
//!
//! # The locked decisions
//!
//! The build brief leaves nine things open. All nine are resolved in
//! `docs/13-deck-conformance.md` §B; five of them shape the code directly:
//!
//! | # | Resolution |
//! |---|---|
//! | 01 | **No on-chain hold.** Two approvals can pass against one card before either settles. The contract is the final arbiter — `settle` re-checks and refuses the second — and the gateway owns the orphan approval. |
//! | 02 | **Per-card rolling limit window**, 30 days by default, reset lazily on read. No keeper. |
//! | 04 | **Declines are recorded**, not merely emitted. A declined `authorize_spend` returns `Ok` and writes a `SpendRecord`. |
//! | 05 | `i128` in the token's smallest unit throughout. `overflow-checks` is on in the release profile, so a subtraction that would go negative traps in the deployed wasm rather than wrapping. |
//! | 08 | TTL bumped on every touched key, **and** the durable history lives in events — the indexer rebuilds all state from the stream alone. |
//!
//! # Two families of refusal
//!
//! An [`errors::Error`] reverts the call. A [`types::ReasonCode`] is written to a successful
//! transaction. A frozen card produces the second, never the first: there is no
//! `Error::CardFrozen`, because a reverted transaction leaves nothing for the evidence table to
//! point at. See the note at the top of [`errors`].
//!
//! # The write discipline
//!
//! Every write function runs the same order, and three steps are load-bearing:
//!
//! ```text
//!  1 require_auth on the correct role
//!  2 validate parameters
//!  3 roll the limit window if it has expired
//!  4 load state
//!  5 guards, cheap → expensive        storage reads before cross-contract calls
//!  6 apply changes
//!  7 assert invariants
//!  8 emit events
//!  9 token transfer                   ALWAYS SECOND-TO-LAST
//! 10 bump TTL                         ALWAYS LAST, and BOUNDED
//! ```
//!
//! **Step 9 second-to-last** is the reentrancy mitigation: a hostile or reentrant token sees a
//! fully consistent, fully committed state. **Step 10** exists because persistent entries
//! archive, and an archived card is a card whose money cannot be moved.
//!
//! # Why every entry point returns `Result`
//!
//! `try_*` on the generated client yields `Err(Ok(Error::X))` only for functions declared to
//! return `Result`. With `panic_with_error!` a test can assert nothing more precise than "it
//! failed", and both the demo script and the indexer branch on the exact number — a test that
//! cannot tell code 6 from code 8 is not testing the thing that matters.
//!
//! # Out of scope, deliberately
//!
//! No card issuer integration, no fiat conversion, no oracle, no KYC, no multi-organisation
//! tenancy, no upgrade path, and **no mainnet deployment**. Phase 1 is testnet only.

pub mod cards;
pub mod errors;
pub mod events;
pub mod invariant;
pub mod roles;
pub mod settle;
pub mod spend;
pub mod storage;
pub mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, String, Vec};

use errors::Error;
use types::{
    Card, CardStatus, Category, Config, Decision, ReasonCode, Role, Settlement, SpendRecord,
    Treasury,
};

#[contract]
pub struct SaintixTreasury;

#[contractimpl]
impl SaintixTreasury {
    // -----------------------------------------------------------------------
    // M1 · config
    // -----------------------------------------------------------------------

    /// Deployment and construction in **one transaction**.
    ///
    /// `stellar contract deploy -- <constructor args>` is the form `deploy.sh` uses, and the
    /// reason is a window rather than a convenience: deploying first and initialising second
    /// leaves an interval in which anybody watching the ledger can call `initialize` and name
    /// themselves admin of a contract somebody else paid to deploy.
    pub fn __constructor(e: Env, admin: Address, token_addr: Address, operators: Vec<Address>) {
        init(&e, &admin, &token_addr, &operators).unwrap_or_else(|_| panic!("already initialised"));
    }

    /// The deck's `initialize`, kept as a real entry point — and it always refuses.
    ///
    /// Because `__constructor` exists the host runs it on every deployment, so by the time any
    /// transaction can reach this function the contract is already configured. That is not a
    /// wasted entry point: the deck's API surface lists `initialize`, M1 asks us to demonstrate
    /// that a second call is refused, and the constructor is what removes the window in which
    /// somebody else could have made the *first* call. Both together mean the specified entry
    /// point exists, behaves as specified, and cannot be raced.
    pub fn initialize(
        e: Env,
        admin: Address,
        token_addr: Address,
        operators: Vec<Address>,
    ) -> Result<(), Error> {
        init(&e, &admin, &token_addr, &operators)
    }

    // -----------------------------------------------------------------------
    // M2 · roles
    // -----------------------------------------------------------------------

    pub fn grant_role(e: Env, caller: Address, who: Address, role: Role) -> Result<(), Error> {
        roles::require(&e, &caller, Role::Admin)?;
        roles::grant(&e, &who, role);
        events::role_set(&e, &who, Some(role));
        storage::bump_instance(&e);
        Ok(())
    }

    pub fn revoke_role(e: Env, caller: Address, who: Address) -> Result<(), Error> {
        roles::require(&e, &caller, Role::Admin)?;
        roles::revoke(&e, &who)?;
        events::role_set(&e, &who, None);
        storage::bump_instance(&e);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // M3 · deposit
    // -----------------------------------------------------------------------

    /// Anyone may fund the treasury; the depositor signs for the transfer.
    ///
    /// `from` is a parameter rather than the invoker for a reason worth keeping in mind: the
    /// contract does not pull money, it asks the token to move money the depositor has already
    /// authorised. There is no token parameter — the token was fixed at construction, which is
    /// what makes "only accepts the configured asset" true by construction rather than by check.
    pub fn deposit(e: Env, from: Address, amount: i128) -> Result<Treasury, Error> {
        from.require_auth();
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let cfg = storage::config(&e).ok_or(Error::NotInitialized)?;

        let mut t = storage::treasury(&e);
        t.total_deposited += amount;
        invariant::assert_books(&t);
        storage::set_treasury(&e, &t);

        events::deposited(&e, &from, amount, &t);

        token::Client::new(&e, &cfg.token).transfer(&from, e.current_contract_address(), &amount);

        storage::bump_instance(&e);
        Ok(t)
    }

    // -----------------------------------------------------------------------
    // M5 · card registry
    // -----------------------------------------------------------------------

    /// `window_secs` of `0` means "use the 30-day default" rather than being an error, so a
    /// caller who has no opinion about decision #02 does not have to have one.
    pub fn create_card(
        e: Env,
        caller: Address,
        category: Category,
        label: String,
        limit: i128,
        window_secs: u64,
    ) -> Result<Card, Error> {
        roles::require(&e, &caller, Role::Admin)?;
        cards::check_label(&label)?;
        cards::check_limit(limit)?;
        let window_secs = if window_secs == 0 {
            cards::DEFAULT_WINDOW_SECS
        } else {
            window_secs
        };
        cards::check_window(window_secs)?;

        let card = Card {
            id: storage::next_card_id(&e),
            category,
            label,
            balance: 0,
            limit,
            limit_window_secs: window_secs,
            window_start: e.ledger().timestamp(),
            spent_in_window: 0,
            status: CardStatus::Active,
        };
        storage::set_card(&e, &card);

        events::card_changed(&e, symbol_short!("create"), &card, &storage::treasury(&e));
        storage::bump_instance(&e);
        Ok(card)
    }

    // -----------------------------------------------------------------------
    // M6 · allocation
    // -----------------------------------------------------------------------

    /// Move money from the unallocated pool onto a card.
    ///
    /// The only guard that matters is the one the deck names `OVER_ALLOCATION`: a card may never
    /// be given money the treasury does not have spare. Everything downstream — every balance
    /// check in `authorize_spend`, every debit in `settle` — assumes this held.
    pub fn allocate(e: Env, caller: Address, card_id: u32, amount: i128) -> Result<Card, Error> {
        roles::require(&e, &caller, Role::Admin)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;
        let mut t = storage::treasury(&e);

        if amount > invariant::unallocated(&t) {
            return Err(Error::OverAllocation);
        }

        t.total_allocated += amount;
        card.balance += amount;
        invariant::assert_books(&t);
        storage::set_treasury(&e, &t);
        storage::set_card(&e, &card);

        events::card_changed(&e, symbol_short!("alloc"), &card, &t);
        storage::bump_instance(&e);
        Ok(card)
    }

    /// Return money from a card to the unallocated pool.
    pub fn deallocate(e: Env, caller: Address, card_id: u32, amount: i128) -> Result<Card, Error> {
        roles::require(&e, &caller, Role::Admin)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;
        if amount > card.balance {
            return Err(Error::InsufficientCardBalance);
        }
        let mut t = storage::treasury(&e);

        t.total_allocated -= amount;
        card.balance -= amount;
        invariant::assert_books(&t);
        storage::set_treasury(&e, &t);
        storage::set_card(&e, &card);

        events::card_changed(&e, symbol_short!("dealloc"), &card, &t);
        storage::bump_instance(&e);
        Ok(card)
    }

    /// Rebalance between two cards. The treasury totals do not move — this is a transfer inside
    /// the allocated pool, so the sum of card balances is unchanged and no solvency question
    /// arises.
    pub fn transfer_between_cards(
        e: Env,
        caller: Address,
        from_id: u32,
        to_id: u32,
        amount: i128,
    ) -> Result<(), Error> {
        roles::require(&e, &caller, Role::Admin)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if from_id == to_id {
            return Err(Error::SameCard);
        }
        let mut from = storage::card(&e, from_id).ok_or(Error::CardNotFound)?;
        let mut to = storage::card(&e, to_id).ok_or(Error::CardNotFound)?;
        if amount > from.balance {
            return Err(Error::InsufficientCardBalance);
        }

        from.balance -= amount;
        to.balance += amount;
        storage::set_card(&e, &from);
        storage::set_card(&e, &to);

        let t = storage::treasury(&e);
        events::card_changed(&e, symbol_short!("xfer_out"), &from, &t);
        events::card_changed(&e, symbol_short!("xfer_in"), &to, &t);
        storage::bump_instance(&e);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // M7 · limits
    // -----------------------------------------------------------------------

    /// Change the ceiling, and optionally the period.
    ///
    /// Changing the period does **not** reset `spent_in_window`. Shortening a window that has
    /// already been spent against would otherwise hand back the whole limit immediately, which
    /// turns a tightening into a loosening.
    pub fn set_limit(
        e: Env,
        caller: Address,
        card_id: u32,
        limit: i128,
        window_secs: u64,
    ) -> Result<Card, Error> {
        roles::require(&e, &caller, Role::Admin)?;
        cards::check_limit(limit)?;
        // Validated before anything is mutated. The host would roll a rejected call back anyway,
        // but a function that half-applies its changes and then decides whether to fail is one
        // reordering away from a real bug.
        if window_secs != 0 {
            cards::check_window(window_secs)?;
        }
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;

        cards::roll_window(&e, &mut card);
        card.limit = limit;
        if window_secs != 0 {
            card.limit_window_secs = window_secs;
        }
        storage::set_card(&e, &card);

        events::card_changed(&e, symbol_short!("limit"), &card, &storage::treasury(&e));
        storage::bump_instance(&e);
        Ok(card)
    }

    // -----------------------------------------------------------------------
    // M8 · status
    // -----------------------------------------------------------------------

    /// Freeze or unfreeze. Either role may call it: an admin freezes as policy, an operator
    /// freezes in response to something happening right now, and making the operator wait for an
    /// admin is the wrong trade in the one case where speed is the point.
    pub fn set_card_status(
        e: Env,
        caller: Address,
        card_id: u32,
        status: CardStatus,
    ) -> Result<Card, Error> {
        roles::require_any(&e, &caller)?;
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;
        card.status = status;
        storage::set_card(&e, &card);

        events::card_changed(&e, symbol_short!("status"), &card, &storage::treasury(&e));
        storage::bump_instance(&e);
        Ok(card)
    }

    // -----------------------------------------------------------------------
    // M9 · spend authorization
    // -----------------------------------------------------------------------

    /// Apply the four rules and record the outcome. **A decline is a success.**
    ///
    /// The `Err` cases here are only the ones where there is nothing sensible to record: the
    /// caller is not an operator, the card does not exist, the amount is not a number of tokens.
    /// Every business refusal — frozen, over limit, short balance, treasury unable to pay —
    /// returns `Ok` with a `Declined` record naming the reason.
    ///
    /// No money moves. `spent_in_window` rises on approval; `balance` does not change until
    /// `settle`.
    pub fn authorize_spend(
        e: Env,
        caller: Address,
        card_id: u32,
        amount: i128,
        merchant_ref: String,
    ) -> Result<SpendRecord, Error> {
        roles::require(&e, &caller, Role::Operator)?;
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if merchant_ref.is_empty() || merchant_ref.len() > 64 {
            return Err(Error::InvalidMerchantRef);
        }
        let cfg = storage::config(&e).ok_or(Error::NotInitialized)?;
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;

        // Step 3 of the discipline. Done before anything is evaluated, so a card whose window
        // lapsed while nobody was calling is judged against the period it is actually in.
        let rolled = cards::roll_window(&e, &mut card);

        let t = storage::treasury(&e);
        // The one cross-contract read, kept behind the three checks that need no network.
        let held = token::Client::new(&e, &cfg.token).balance(&e.current_contract_address());

        let reason = spend::evaluate(&card, amount, &t, held);
        let decision = match reason {
            None => Decision::Approved,
            Some(_) => Decision::Declined,
        };

        if decision == Decision::Approved {
            card.spent_in_window += amount;
        }
        if rolled || decision == Decision::Approved {
            storage::set_card(&e, &card);
            events::card_changed(&e, symbol_short!("spend"), &card, &t);
        }

        let record = SpendRecord {
            id: storage::next_spend_id(&e),
            card_id,
            amount,
            merchant_ref,
            decision,
            reason_code: reason.unwrap_or(ReasonCode::None),
            settled: false,
            ledger_seq: e.ledger().sequence(),
            timestamp: e.ledger().timestamp(),
        };
        storage::set_spend(&e, &record);
        events::spend_recorded(&e, &record);

        storage::bump_instance(&e);
        Ok(record)
    }

    // -----------------------------------------------------------------------
    // M10 · whitelist
    // -----------------------------------------------------------------------

    pub fn add_whitelist(e: Env, caller: Address, who: Address) -> Result<(), Error> {
        roles::require(&e, &caller, Role::Admin)?;
        // The contract may not be its own settlement destination. Paying itself is a no-op
        // transfer whose bookkeeping still runs, which strands the amount: off the card, never
        // back into unallocated, and beyond the reach of every entry point. Caught here, where
        // the mistake is made, rather than later where the money goes.
        if who == e.current_contract_address() {
            return Err(Error::InvalidDestination);
        }
        let mut w = storage::whitelist(&e);
        if w.contains_key(who.clone()) {
            return Err(Error::WhitelistUnchanged);
        }
        w.set(who.clone(), ());
        storage::set_whitelist(&e, &w);
        events::whitelist_changed(&e, &who, true);
        storage::bump_instance(&e);
        Ok(())
    }

    /// Removing an address does **not** touch settlements already made to it. A reconciliation
    /// record is a statement about what happened, and rewriting history to match current policy
    /// would make the ledger useless as evidence.
    pub fn remove_whitelist(e: Env, caller: Address, who: Address) -> Result<(), Error> {
        roles::require(&e, &caller, Role::Admin)?;
        let mut w = storage::whitelist(&e);
        if !w.contains_key(who.clone()) {
            return Err(Error::WhitelistUnchanged);
        }
        w.remove(who.clone());
        storage::set_whitelist(&e, &w);
        events::whitelist_changed(&e, &who, false);
        storage::bump_instance(&e);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // M11 · M12 · withdraw and reconcile
    // -----------------------------------------------------------------------

    /// Pay an approved spend out to a whitelisted address, debiting the card that incurred it,
    /// and record the link.
    ///
    /// This is the only entry point that moves money out. It is also the one a reviewer will
    /// open first: the settlement transaction hash plus the `spend_id` on the record is the
    /// whole of Deliverable 3's evidence.
    pub fn settle(
        e: Env,
        caller: Address,
        spend_id: u32,
        to: Address,
        amount: i128,
    ) -> Result<Settlement, Error> {
        roles::require(&e, &caller, Role::Operator)?;
        let cfg = storage::config(&e).ok_or(Error::NotInitialized)?;
        // Defence in depth against a whitelist written before `add_whitelist` guarded this. The
        // check costs one comparison and closes the only path by which money strands itself.
        if to == e.current_contract_address() {
            return Err(Error::InvalidDestination);
        }

        let mut record = storage::spend(&e, spend_id).ok_or(Error::SpendNotFound)?;
        let mut card = storage::card(&e, record.card_id).ok_or(Error::CardNotFound)?;
        let whitelisted = storage::whitelist(&e).contains_key(to.clone());
        let existing = storage::settlement_of_spend(&e, spend_id);

        settle::check(&record, &card, amount, existing, whitelisted)?;

        let mut t = storage::treasury(&e);
        card.balance -= amount;
        t.total_settled += amount;
        record.settled = true;
        invariant::assert_books(&t);

        let s = Settlement {
            id: storage::next_settlement_id(&e),
            spend_id,
            card_id: card.id,
            amount,
            to: to.clone(),
            ledger_seq: e.ledger().sequence(),
            timestamp: e.ledger().timestamp(),
        };

        storage::set_treasury(&e, &t);
        storage::set_card(&e, &card);
        storage::set_spend(&e, &record);
        storage::set_settlement(&e, &s);

        events::card_changed(&e, symbol_short!("settle"), &card, &t);
        events::spend_recorded(&e, &record);
        events::settled(&e, &s, &t);

        // Second-to-last, after every write has landed. A reentrant token sees a fully
        // consistent state in which this spend is already settled.
        token::Client::new(&e, &cfg.token).transfer(&e.current_contract_address(), &to, &amount);

        storage::bump_instance(&e);
        Ok(s)
    }

    // -----------------------------------------------------------------------
    // Views
    // -----------------------------------------------------------------------

    pub fn get_config(e: Env) -> Result<Config, Error> {
        storage::config(&e).ok_or(Error::NotInitialized)
    }

    /// The three running totals. `get_unallocated` is the fourth number a dashboard wants and it
    /// is derived, not stored — see [`invariant`].
    pub fn get_treasury(e: Env) -> Treasury {
        storage::treasury(&e)
    }

    pub fn get_unallocated(e: Env) -> i128 {
        invariant::unallocated(&storage::treasury(&e))
    }

    /// What the sum of every card balance must equal. The test harness compares it against the
    /// actual sum; a mismatch is an accounting bug the running totals cannot see.
    pub fn get_allocated_outstanding(e: Env) -> i128 {
        invariant::allocated_outstanding(&storage::treasury(&e))
    }

    pub fn list_cards(e: Env) -> Vec<Card> {
        storage::list_cards(&e)
    }

    pub fn get_card(e: Env, card_id: u32) -> Result<Card, Error> {
        storage::card(&e, card_id).ok_or(Error::CardNotFound)
    }

    /// The card as it would be evaluated **right now**, with an expired window already rolled.
    ///
    /// A view that returned the stored `spent_in_window` would tell a console last period's
    /// figure and let it warn about a limit that has already reset. It does not write — the roll
    /// is persisted by the next call that changes anything.
    pub fn get_card_live(e: Env, card_id: u32) -> Result<Card, Error> {
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;
        cards::roll_window(&e, &mut card);
        Ok(card)
    }

    pub fn get_remaining_limit(e: Env, card_id: u32) -> Result<i128, Error> {
        let mut card = storage::card(&e, card_id).ok_or(Error::CardNotFound)?;
        cards::roll_window(&e, &mut card);
        Ok(cards::remaining_limit(&card))
    }

    /// Every spend, or every spend on one card. `card_id` of `0` means all — ids start at 1, so
    /// zero is unambiguous.
    pub fn list_spends(e: Env, card_id: u32) -> Vec<SpendRecord> {
        storage::list_spends(&e, if card_id == 0 { None } else { Some(card_id) })
    }

    pub fn get_spend(e: Env, spend_id: u32) -> Result<SpendRecord, Error> {
        storage::spend(&e, spend_id).ok_or(Error::SpendNotFound)
    }

    /// The reconciliation ledger, and the source of the evidence table.
    pub fn list_settlements(e: Env) -> Vec<Settlement> {
        storage::list_settlements(&e)
    }

    pub fn get_settlement(e: Env, id: u32) -> Result<Settlement, Error> {
        storage::settlement(&e, id).ok_or(Error::SettlementNotFound)
    }

    /// The settlement that closed a given spend, if any. The reverse lookup a reviewer follows.
    pub fn get_settlement_of_spend(e: Env, spend_id: u32) -> Option<Settlement> {
        storage::settlement_of_spend(&e, spend_id).and_then(|id| storage::settlement(&e, id))
    }

    pub fn is_whitelisted(e: Env, who: Address) -> bool {
        storage::whitelist(&e).contains_key(who)
    }

    pub fn list_whitelist(e: Env) -> Vec<Address> {
        storage::whitelist(&e).keys()
    }

    pub fn role_of(e: Env, who: Address) -> Option<Role> {
        storage::roles(&e).get(who)
    }
}

/// Shared by `__constructor` and `initialize` so there is exactly one definition of what a
/// constructed contract looks like.
fn init(
    e: &Env,
    admin: &Address,
    token_addr: &Address,
    operators: &Vec<Address>,
) -> Result<(), Error> {
    if storage::has_config(e) {
        return Err(Error::AlreadyInitialized);
    }

    // Read from the token, never taken from the deployer. A wrong `decimals` in config would
    // make every amount an explorer renders wrong by orders of magnitude, and nothing on chain
    // would contradict it.
    let decimals = token::Client::new(e, token_addr).decimals();

    storage::set_config(
        e,
        &Config {
            founder: admin.clone(),
            token: token_addr.clone(),
            decimals,
        },
    );
    storage::set_treasury(
        e,
        &Treasury {
            total_deposited: 0,
            total_allocated: 0,
            total_settled: 0,
        },
    );

    roles::grant(e, admin, Role::Admin);
    for op in operators.iter() {
        roles::grant(e, &op, Role::Operator);
    }

    events::initialized(e, admin, token_addr, decimals);
    storage::bump_instance(e);
    Ok(())
}
