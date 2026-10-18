#![no_std]
//! A SEP-41 token with settable decimals and a clawback. **Testnet and test fixtures only.**
//!
//! # Why not the Stellar Asset Contract
//!
//! On testnet the SAC is the right answer for USDC and this crate is not a replacement for it —
//! `deploy.sh` takes a token address and will happily take the real one. What the SAC cannot do
//! is put the treasury into the one state the SOW asks us to demonstrate.
//!
//! # The clawback is the point
//!
//! Test case T5 requires a spend declined with `TREASURY_INSOLVENT`: the card has balance and
//! limit to spare, and the contract still cannot pay. If the contract's own accounting were the
//! only thing consulted, that state would be unreachable — the accounting identity guarantees
//! the treasury covers every card. It becomes reachable only when the tokens leave without the
//! contract's knowledge, which is exactly what an issuer clawback does, and Stellar assets can
//! have clawback enabled.
//!
//! So `authorize_spend` reads the token's real balance rather than trusting its own books, and
//! [`MockToken::clawback`] is how a test creates the gap. Without it T5 could only be written by
//! corrupting storage, which would test nothing about the contract.
//!
//! # What it deliberately does not have
//!
//! No allowances, no authorisation on `mint` or `clawback`, no admin, no burn-by-holder. Anyone
//! can mint. That is catastrophic for a real token and exactly right for a fixture one: the demo
//! script funds several accounts without a key-management ceremony, and a token nobody could
//! mint from would need one.
//!
//! **Nothing in this crate may ever reach mainnet.** The treasury contract takes its token by
//! address and never links against this crate, so the only thing standing between it and a real
//! deployment is not deploying it.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, MuxedAddress};

#[contracttype]
enum Key {
    Decimals,
    Balance(Address),
}

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn __constructor(e: Env, decimals: u32) {
        e.storage().instance().set(&Key::Decimals, &decimals);
    }

    /// Read by the treasury's constructor, which never trusts the decimals its caller passes.
    pub fn decimals(e: Env) -> u32 {
        e.storage().instance().get(&Key::Decimals).unwrap_or(7)
    }

    pub fn balance(e: Env, id: Address) -> i128 {
        e.storage().persistent().get(&Key::Balance(id)).unwrap_or(0)
    }

    /// Unauthenticated on purpose. See the module note.
    pub fn mint(e: Env, to: Address, amount: i128) {
        let b = Self::balance(e.clone(), to.clone());
        e.storage()
            .persistent()
            .set(&Key::Balance(to), &(b + amount));
    }

    /// Remove tokens from a holder without their signature — what an issuer with clawback
    /// enabled can do, and the only way to reach `TREASURY_INSOLVENT` honestly.
    pub fn clawback(e: Env, from: Address, amount: i128) {
        let b = Self::balance(e.clone(), from.clone());
        if b < amount {
            panic!("clawback exceeds balance");
        }
        e.storage()
            .persistent()
            .set(&Key::Balance(from), &(b - amount));
    }

    /// `MuxedAddress` because that is what SEP-41 declares in SDK 27, and the treasury calls
    /// through the generated SEP-41 client — a signature that differs by that much does not link.
    ///
    /// # The self-transfer case is not a formality
    ///
    /// This function used to read both balances, then write both. When `from == to` that reads
    /// the same number twice and the second write wins, so `transfer(x, x, n)` **created `n`
    /// tokens out of nothing**.
    ///
    /// In a fixture token that anybody can `mint` from, minting is not itself the harm. The harm
    /// is that `check_books` — the assertion every one of the treasury's tests ends with —
    /// compares the treasury's accounting against *this token's* balances. A token that quietly
    /// inflates is a token that can hide a real accounting bug in the contract under test, which
    /// is the one thing a fixture must never do.
    pub fn transfer(e: Env, from: Address, to: MuxedAddress, amount: i128) {
        from.require_auth();
        let to = to.address();
        let fb = Self::balance(e.clone(), from.clone());
        // Panics on an overdraft rather than going negative. A fixture token that let a balance
        // go below zero would let the acceptance scenario "succeed" through a transfer that
        // could not happen against a real token.
        if fb < amount {
            panic!("insufficient balance");
        }
        if from == to {
            // A no-op, and it must be written as one. Falling through would double-read.
            return;
        }
        let tb = Self::balance(e.clone(), to.clone());
        e.storage()
            .persistent()
            .set(&Key::Balance(from), &(fb - amount));
        e.storage()
            .persistent()
            .set(&Key::Balance(to), &(tb + amount));
    }
}
