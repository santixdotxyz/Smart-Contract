# Demo runbook

Everything needed to present Saintix SOW 1, in the order to present it.

---

## Before you start

**Do not run `make demo` again once you start recording.** It deploys a *fresh* treasury and
rewrites `smart-contract/.deploy/` and the README, so your recording would cite a contract that no longer
matches the committed files.

Use **`make verify`** instead. It is read-only, needs no key, deploys nothing, and can be run as
many times as you like — including live, on camera.

```bash
git status          # should be clean
make verify         # ~30s, proves the committed evidence is real
```

---

## The five minutes

### 1 · What the contract is (30s)

> An organisation's USDC treasury on chain, split into category virtual cards — SaaS, AI,
> Advertising, Cloud — each with its own balance and its own spending limit. Every authorization
> decision is recorded on chain, and every settlement links back to the spend that caused it.

Show `README.md`, the box under **What it does**.

**The one architectural point worth making:** the contract is a rule book and a ledger, not a
payment gateway. A card network needs an answer in about two seconds; a Stellar ledger closes in
about five. So authorization happens off-chain against on-chain constraints, and reconciles back.

### 2 · Run the verifier live (60s)

```bash
make verify
```

Let it run on screen. Narrate while it works:

> This is not the script that produced the evidence — that one needs a key and deploys contracts.
> This one only reads. It takes every transaction hash in the committed evidence file and asks a
> Stellar node whether it actually succeeded.

Point at the verdict: **23 transactions verified · 3 recorded declines · 6 labelled refusals.**

### 3 · Open one transaction (45s) — **the strongest moment**

From `smart-contract/.deploy/evidence.md`, click **the row marked `CardFrozen`**.

Stellar Expert shows the transaction **succeeded**. Say why that matters:

> This is a *declined* spend, and it is a **successful transaction**. That is deliberate. A
> refusal that reverted would leave nothing on chain to open — and the SOW requires a reviewer
> with minimal technical expertise to see *why* a spend was refused. So the contract records the
> refusal instead of throwing it away.

Then contrast with a `simulation` row:

> This one was refused too, but it never reached the ledger — the CLI simulates before submitting.
> So there is no hash, and we label it rather than inventing one. A fabricated hash is the one
> thing a reviewer would have no way to catch.

### 4 · Follow the money (60s)

Click **the settle row** — 2,400 USDC leaves the contract.

Then open **`smart-contract/.deploy/reconciliation.md`**. Find row `sp_1`:

> The same transaction, linked back to the spend the SaaS card authorised. That link is what turns
> a bare transfer into an accountable payment — you can say which cost category it came from.

Point at the two figures per card — **balance and limit are separate columns**:

> A card can be rich and out of limit, or well under its limit and broke. They are two independent
> constraints, and the contract returns two different reason codes for them.

### 5 · The tests (45s)

```bash
make test
```

> 128 contract tests, 18 for the off-chain indexer, 96.5% line coverage. Every one of the ten test
> cases the SOW names has a test named after it — `t1_approved_spend` through
> `t10_settlement_amount_and_double_settle`.

Worth mentioning if asked about rigour:

> Every scenario ends by summing the actual cards and comparing against the token's real balance —
> not the contract's own bookkeeping. That is where a missing transfer would hide.

---

## If you are asked

| Question | Answer |
|---|---|
| Is this on mainnet? | **No, and deliberately.** Phase 1 is testnet only; mainnet is deferred until after a security review that has not happened. |
| Where is the frontend? | Out of scope — SOW §4.1 excludes the frontend and backend application layer. The evidence package replaces the demo console; see [`docs/13-deck-conformance.md`](docs/13-deck-conformance.md) §B-6. |
| What is `backend/`? | A **read-only** command-line indexer — no server, no database, no keys. Not the application layer §4.1 excludes. |
| Can I verify this myself? | Yes: `make verify`, or `cd backend && cargo run --release -- report --contract <id>`. Neither needs a key. |
| Where are the screenshots and video? | The evidence package stands in for them: every claim is a transaction hash anyone can open on a third-party explorer, and `make verify` re-checks all of them live against the chain. |
| What is *not* done? | The authorization gateway (Phase 2), capture/void/refund, multi-organisation tenancy, and the security audit. All listed in [`docs/12-roadmap.md`](docs/12-roadmap.md). |

---

## Suggested screen captures

If a still image is wanted for each deliverable:

| For | Capture |
|---|---|
| D1 | `make verify` output, beside the deposit row open on Stellar Expert |
| D2 | the three declined rows, and one of them open showing its reason code |
| D3 | the settle transaction open, beside `reconciliation.md` row `sp_1` |

---

## Regenerating everything

Only if you need a fresh run. **Not while recording.**

```bash
stellar keys generate --fund sx-deployer --network testnet   # once
KEY=sx-deployer make demo     # deploys, runs 30 steps, rewrites smart-contract/.deploy/ and README
make coverage                 # the third evidence file
make verify                   # confirm the new package
git add -A && git commit -m "evidence: fresh testnet run"
```
