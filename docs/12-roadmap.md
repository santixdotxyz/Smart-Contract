# 12 · Roadmap

What Phase 1 deliberately left open, and what has to happen before any of this touches real money.

## Before mainnet — not negotiable

### A security audit

Out of scope for Phase 1 by SOW §4.1, and the reason mainnet is deferred rather than merely unscheduled. Everything below is secondary to it.

The two things an audit should be pointed at first:

- **The admin key is the whole treasury.** An admin can allocate the entire pool onto one card, whitelist any address, and grant itself the operator role. There is no mitigation in the contract and there should not be one — the mitigation is operational. See below.
- **`settle` is the only path money leaves by**, and its nine gates are the entire boundary. They are covered by fifteen tests and the demo drives four of them against a live network, which is evidence and not proof.

### Multisig for the admin

`deploy.sh` already takes the admin and operator addresses separately, so this needs no contract change: point `--admin` at a Stellar multisig account.

It should be the default for any real deployment, and it is the single highest-value change on this list. It converts "one stolen key is the treasury" into "one stolen key is nothing".

### Operator key handling

Decision #06 accepts a hot operator key because it has to answer in seconds. What bounds it today is the whitelist — an operator can move money, but only to somewhere an admin already approved.

What Phase 2 should add is rotation without downtime, which the contract already supports (`grant_role` a new operator, `revoke_role` the old one, in either order) but nothing operationalises.

## Phase 2 · The gateway

The largest missing piece, and the one the whole architecture is shaped around. [09-offchain-integration.md](09-offchain-integration.md) is its handover.

Three things it must own that the contract deliberately does not:

**A reservation model.** The contract holds nothing on authorization (decision #01), so the gateway must subtract approved-but-unsettled amounts from the balance it believes a card has. Without it, two swipes against one card both get approved and one of them cannot settle.

**Orphan reconciliation.** An approval the contract later refuses to settle is visible as an approved, unsettled `SpendRecord`. Somebody has to either re-settle it against a topped-up card or void it with the merchant. Today nothing does.

**Staleness discipline.** The gateway's cache is at least one ledger behind. It must know how far behind and decline conservatively past a threshold, or it will approve spending against limits that no longer exist.

The recommended route for the first of these is to **link against `smart-contract/src/spend.rs::evaluate`** rather than reimplement the four checks. It is a pure function for that reason. Two implementations of the same rules, written from prose, will differ — and the way they differ is that a customer is told one reason and the ledger records another.

## Phase 2 · Contract work

Ordered by how much a real deployment would miss them.

### Capture, void and refund

Phase 1 models `Approved | Declined` and a `settled` flag. A real card network has authorization then **capture**, and the captured amount can differ from the authorised one — a tip, a partial capture — plus void, refund and chargeback.

The deck asks (slide 15) that we not build these in Phase 1 but leave room. We did: `Decision` is an enum rather than a boolean, so `Captured` is a code change rather than a storage migration, and `settle` already allows settling for **less** than the spend.

What is genuinely missing is a *running remainder*. Today a spend closes at whatever figure it closed for and the rest is simply not owed. Partial capture with a remainder needs a new field and is a migration.

### Paging on the list views

`list_cards`, `list_spends` and `list_settlements` walk from 1 to the sequence value. On a treasury with tens of thousands of spend records, `list_spends(0)` will not fit in a transaction.

Accepted for Phase 1 — these are views, they cost nothing on chain, and the tool that needs full history reads events instead. A console would need `list_spends(card_id, from, limit)`.

### A card that can be closed

There is no `archive_card`. A category that is no longer used sits in `list_cards` forever, and its id is never reused.

That is correct for the ledger — a removed card would orphan its spend history — but a `Closed` status that keeps the record while removing it from the working set is the obvious addition.

### Per-card allowlists

A card is bound to a *category*, not to a merchant. Nothing stops an approved operator authorising an advertising spend against the SaaS card; only the label says which is which.

A merchant allowlist per card is the natural next control, and it is the kind of thing that belongs on chain because it is a rule, not a decision.

## Phase 2 · Multi-organisation

Explicitly out of scope in Phase 1, and the deck is right that it should be: one contract per organisation is the simplest thing that works, and it makes tenancy bugs structurally impossible rather than merely tested against.

What it costs is discoverability. There is no registry, so somebody holding an organisation's name has no way to find its treasury. Two ways out, and they are not equivalent:

- **A factory contract** that deploys instances and records them. Discoverable, and it makes the deployer a party to every treasury.
- **An off-chain registry.** Keeps the contracts independent; makes the registry a thing that can be wrong.

The first is what the deck floats as decision #07. It is worth deciding on the basis of who is expected to run the registry, not on which is easier to build.

## What will not change

Written down so a future change has to argue against it rather than drift past it.

**Governance stays absent.** No upgrade path, no admin override, no pause. Each would be useful in an emergency and each is a key that can be stolen. The answer to a compromised key is to bound what any key can do, not to add a stronger one.

**Declines stay on chain.** Decision #04 costs storage and buys the §6 evidence requirement. A future cost optimisation that moves declines to events alone removes the transaction a reviewer is supposed to open.

**The four checks stay in that order.** It is part of the interface, not an implementation detail, because the gateway reproduces it.

**The card is debited at settlement, not at authorization.** Debiting on authorization leaves a card short for a payment that never happened.

**Balance and limit stay separate.** Merging them is the design mistake that is hardest to undo, and it is hardest to undo because everything downstream — reports, alerts, the console's two bars — will have been written against the merged figure by the time anybody notices.

## Milestone criteria

What "done" means for each of the next steps, in the same style as the deck's module table.

| | Done when |
|---|---|
| **Security audit** | A report exists, findings are triaged, and every *critical* and *high* is either fixed or accepted in writing with a reason. |
| **Multisig admin** | A treasury is deployed with a multisig admin on testnet and `make demo` passes against it unchanged. |
| **Gateway** | It approves and declines against cached state in under 2s at the target rate, links against `spend::evaluate`, and its decision log reconciles against `list_spends` with zero unexplained rows. |
| **Capture** | `SpendRecord` carries a remainder, partial capture is tested to the smallest unit, and the invariants hold across a capture-then-refund sequence. |
| **Mainnet** | Everything above, plus real USDC, plus an evidence run on mainnet with the same script and the same assertions. |
