# Saintix — on-chain treasury with category virtual cards

A Soroban contract layer that holds an organisation's USDC treasury, splits it into **category-based virtual cards** each with its own balance and spending limit, records every spend authorization decision on chain, and settles approved spends to whitelisted addresses with a reconciliation record linking each payment back to the spend that caused it.

**Instaward Phase 1 · Stellar testnet · smart contract layer only.**

> **There is no mainnet deployment, and there will not be one in Phase 1.** Mainnet is deferred until after a security review that has not happened. Every contract address below is a testnet address, and the token in the default demo path is a fixture anyone can mint.

---

## What it does

A company holding stablecoin still pays its SaaS, AI, advertising and cloud bills in fiat. Bridging that is a solved problem and somebody else's. The problem Saintix solves is the second one: **allocating, tracing and controlling spending across cost categories** — which is not a bridge, but a state machine for money. That state machine is what this contract puts on chain.

```
Deploy      one contract per ORGANISATION. No factory, no tenancy, no shared state
Roles       admin (shape of the money) · operator (movement of the money)
Asset       one SEP-41 token, fixed at construction. USDC on Stellar: 7 decimals
Treasury    three running totals; every other figure is derived from them
Cards       category · balance · limit · rolling window · status
Spend       four checks in a frozen order. Approved AND declined are recorded on chain
Settlement  operator only · whitelisted destination only · at most the spend · once
Governance  no upgrade path, no admin override, no pause
```

### The architectural claim everything follows from

**The contract is a rule book and a ledger, not a payment gateway.** A card network wants an answer in about two seconds; a Stellar ledger closes in about five. So the contract is where the rules are *defined* and where decisions are *recorded* — an off-chain gateway reads its state and answers the network in milliseconds, then writes back at least one ledger later.

Two consequences run through the design: `authorize_spend` moves no money, and there is a race window that is [accepted rather than engineered away](docs/07-invariants-and-security.md).

---

## Quickstart

```bash
# prerequisites: rust 1.97.1, the stellar CLI, jq, curl

make all                        # fmt, clippy -D warnings, test, build — both trees
make demo                       # the acceptance scenario, end to end on testnet
make coverage                   # the third evidence file — no key, no network
```

On testnet, `make demo` creates and funds its own signer (`sx-deployer`) if you have not given it
one, the same way it already creates the five role identities. Pass `KEY=<alias>` to use a signer
of your own — see [Providing the signing key](#providing-the-signing-key). On `public` nothing is
generated and `KEY` is required.

`make demo` deploys a fresh treasury, drives thirty steps through it, and writes `smart-contract/.deploy/evidence.md` with a transaction hash for every step that reached the ledger.

### Checking it without running it

```bash
make verify      # no key, deploys nothing, changes nothing — safe to run live
```

`make verify` takes the **committed** evidence, pulls every transaction hash out of it and asks a
Stellar node whether each one actually succeeded, then rebuilds the treasury from its event stream
and confirms the reconciliation still holds. It is a different code path from the one that
produced the evidence, on purpose: the demo makes the claim, this audits it.

**Presenting this? → [`DEMO.md`](DEMO.md)** — a five-minute runbook, in English and Vietnamese.

---

## Providing the signing key

`make demo` and `make deploy` submit real transactions, so they need something that can **sign**. That is the `KEY` variable, and it accepts either form:

| | `KEY=` | Where the secret lives |
|---|---|---|
| **An identity alias** | `sx-deployer` | `~/.config/stellar/identity/sx-deployer.toml`, written by the Stellar CLI, mode `0600` |
| **A raw secret key** | `SC36…` (56 chars, starts with `S`) | Nowhere on disk — only in the environment of that one command |

Both are resolved by the Stellar CLI itself; the scripts never parse or store a key.

### On testnet, you can leave it out

```bash
make demo        # generates and funds sx-deployer if it does not exist, then uses it
```

The deployer used to be the one account a reviewer had to create by hand before anything would
run, in a scenario whose whole claim is *clone it, run one command*. It is now bootstrapped like
the five role identities: friendbot XLM, a throwaway alias, nothing to lose. `preflight` says so
on the way past, so a run never signs with a key you did not know about.

**This happens on test networks only.** With `NETWORK=public` an absent `KEY` is still a hard
failure — there is no friendbot there, and a silently generated key would own a real deployment
while existing only in a file nobody was told about.

### Naming your own — an alias

```bash
stellar keys generate --fund sx-deployer --network testnet
KEY=sx-deployer make demo
```

`--fund` asks friendbot for testnet XLM. The account needs a balance because it pays the fees for the two contract deployments and the thirty steps.

### Injecting a secret directly — CI, or a machine you would rather not write keys to

```bash
KEY=SC36…  make demo          # works, but see the warning below
```

Keep it out of your shell history by reading it instead of typing it:

```bash
read -rs KEY && export KEY    # paste, press enter; nothing is echoed or recorded
make demo
```

From a file or a secret manager:

```bash
export KEY="$(cat ~/.secrets/saintix-deployer)"
export KEY="$(op read op://vault/saintix/deployer)"     # 1Password, as an example
```

In GitHub Actions:

```yaml
- run: KEY=$KEY make demo
  env:
    KEY: ${{ secrets.SAINTIX_TESTNET_DEPLOYER }}
```

> **A secret on a command line is visible to every process on the machine** via `ps`, and lands in `~/.zsh_history` or `~/.bash_history`. `read -rs` or an environment variable set from a secret store avoids both. This matters less on testnet than it would elsewhere — see below — but the habit is worth keeping.

### Common mistakes, and what they now say

| You passed | What happens |
|---|---|
| nothing | Refuses immediately, and prints both ways to supply it |
| a **public** key `G…` | Refuses immediately: *"It identifies an account but cannot sign for it"* — a keypair has two halves and only one signs |
| a muxed address `M…` | Refuses immediately |
| an alias that does not exist | Refuses immediately, with the `stellar keys generate` line to run |
| an unfunded account | Reaches the first deploy, then fails on fees. Run `stellar keys fund <alias> --network testnet` |

All of these are caught by `preflight` in [`smart-contract/scripts/lib.sh`](smart-contract/scripts/lib.sh) **before any transaction is submitted**.

### What the demo does with keys of its own

`demo.sh` needs five participants with distinct roles — admin, operator, outsider, settlement, rogue — because a guard is only proven by somebody who is *not* allowed past it. It creates them itself:

```
sx_admin  sx_operator  sx_outsider  sx_settlement  sx_rogue
```

These are **generated and funded by friendbot on the machine that runs the demo**, and they are written to `~/.config/stellar/identity/`. Nothing about them is secret or valuable, and re-running the demo reuses them. Delete them with `stellar keys rm sx_admin` and so on.

### Security

**These are testnet keys.** Friendbot hands out the XLM, the token in the default path is a fixture anyone can mint, and there is [no mainnet deployment in Phase 1](#out-of-scope) — so a leaked testnet deployer key costs nothing but a re-run.

**Never point `KEY` at an account holding real funds.** Nothing in this repository needs one, and there is no mainnet path that would use it.

The Stellar CLI stores aliases as plaintext seed phrases in `~/.config/stellar/identity/*.toml` (mode `0600`). If that is not acceptable on a given machine, use the raw-secret route above and write nothing to disk.

---

## Evidence of completion

### 📁 The files are in [`smart-contract/.deploy/`](smart-contract/.deploy/) — start at [`smart-contract/.deploy/README.md`](smart-contract/.deploy/README.md)

Nothing needs to be installed or run to check them:

| | |
|---|---|
| [`smart-contract/.deploy/evidence.md`](smart-contract/.deploy/evidence.md) | one row per step, with the transaction — **§6.2 is ticked against this** |
| [`smart-contract/.deploy/reconciliation.md`](smart-contract/.deploy/reconciliation.md) | the same treasury rebuilt independently from its events |
| [`smart-contract/.deploy/coverage.txt`](smart-contract/.deploy/coverage.txt) | what the test suite actually reaches |

> **Use the links above rather than browsing for the directory.** `.deploy/` is hidden: it does
> not appear in an IDE file tree, in Finder, or in `ls` without `-a`. The links work normally on
> GitHub and in any editor that follows them. The package lives beside the run that produced it;
> everything else in `.deploy/` is build scratch and none of it is worth opening.

The same table is reproduced below. Generated by `make demo`, not transcribed by hand — every hash
opens on Stellar Expert.

<!-- BEGIN EVIDENCE-HEADER -->
| | |
|---|---|
| Network | testnet |
| Treasury contract | [`CBIB6RTTER6WGHFRAM5FIMKA6BCQZ27WV435Z3PQS2IXSKNHBK35B7FS`](https://stellar.expert/explorer/testnet/contract/CBIB6RTTER6WGHFRAM5FIMKA6BCQZ27WV435Z3PQS2IXSKNHBK35B7FS) |
| Token (stand-in USDC, 7 decimals) | [`CAEABYM5LXKOWWHXJQQILVFNH2EDBIGCN2NVICW7EE4ANN64YCN3O2MA`](https://stellar.expert/explorer/testnet/contract/CAEABYM5LXKOWWHXJQQILVFNH2EDBIGCN2NVICW7EE4ANN64YCN3O2MA) |
| Admin | `GBCX2RVX3FITYQGTMO56UI6OJ5V6QNQYHR4BNMZ47E33JSGIII6T2HEU` |
| Operator | `GALKYGS6A7DOC7DDD2VMS42LOWTLIMZP2PEIAW2KRHL3M2I2YYE6LKEQ` |
| Settlement address (whitelisted) | `GCM222FS6QJ3HF5JV2I2IXDGFTL32KQTPPCNXG5IW46NWJRR526QFT6K` |
<!-- END EVIDENCE-HEADER -->

### Three kinds of row, and the difference is the point

| Row | Means |
|---|---|
| a hash, no code | The call succeeded. |
| a hash **and** a code in bold | The spend was **declined and recorded**. A decline is a successful transaction here — that is what lets you open it and read the reason. |
| `simulation` | The contract refused and the call never reached the ledger. The CLI simulates before submitting, so there is no transaction to link. Labelled rather than given a hash, because a fabricated hash is the one artefact a reviewer would have no way to catch. |

**Every refusal below was matched against its exact error or reason code.** A step that failed for the *wrong* reason would have stopped the run and produced no file.

### Deliverable 1 · On-Chain Treasury

<!-- BEGIN EVIDENCE-D1 -->
| # | Step | Code | Transaction |
|---|---|---|---|
| 1 | deposit 250,000 USDC into the treasury | — | [`878ae3600061…`](https://stellar.expert/explorer/testnet/tx/878ae360006174fb21171f3cc0723484649b51994a8349f381c59c4128541675) |
<!-- END EVIDENCE-D1 -->

*Proves: a treasury contract deployed on testnet for one organisation, receiving stablecoin and reporting the correct available balance. The chain was queried after the deposit and asserted to say 250,000 deposited and 250,000 unallocated.*

### Deliverable 2 · Category Cards & Spending Control

<!-- BEGIN EVIDENCE-D2 -->
| # | Step | Code | Transaction |
|---|---|---|---|
| 2 | create card · saas | — | [`9050c6561633…`](https://stellar.expert/explorer/testnet/tx/9050c6561633b281a17ad6754d1bce80fe293db3f2445bd219184a2506f91b1a) |
| 3 | create card · ai-tools | — | [`a06f5254d8fa…`](https://stellar.expert/explorer/testnet/tx/a06f5254d8fa03de738e3ac0573f6c0b25f3a840e61b8decc56eb1e4f47bf9c0) |
| 4 | create card · advertising | — | [`9371b42460a0…`](https://stellar.expert/explorer/testnet/tx/9371b42460a02595db38c65c80c1af75d76680c8aeff6c347e22303f81906df7) |
| 5 | create card · cloud | — | [`36bcc10b09d4…`](https://stellar.expert/explorer/testnet/tx/36bcc10b09d4c612b89cac8ce6fa2aae0c0383b377572b66acc93d098c6bc5d7) |
| 6 | allocate 42,300 to saas | — | [`c2a7c7101a31…`](https://stellar.expert/explorer/testnet/tx/c2a7c7101a31a58565830789b8e9669a603d84e5a34246737e720e4c3162c152) |
| 7 | allocate 61,000 to ai-tools | — | [`a48a9368da9b…`](https://stellar.expert/explorer/testnet/tx/a48a9368da9b69753a494bcad8521175ba72b570596d59c46dc5b8829d1f0bfa) |
| 8 | allocate 58,400 to advertising | — | [`59dd7f6ab28a…`](https://stellar.expert/explorer/testnet/tx/59dd7f6ab28a325b9a847fea8c93a07236ff4983efafcaa9af04a52b8e5d2365) |
| 9 | allocate 25,000 to cloud | — | [`55fa6c35e191…`](https://stellar.expert/explorer/testnet/tx/55fa6c35e19183b9c2becbd0877aa667f7347998ce38b82b1166b38e9ca44082) |
| 10 | deallocate 4,200 from advertising back to the pool | — | [`59d7ee9ebb58…`](https://stellar.expert/explorer/testnet/tx/59d7ee9ebb586887990ab052a6a0fdc38984f2650da846443e2f12fe863105a7) |
| 11 | rebalance 1,000 from ai-tools to saas | — | [`4ac78a36c70f…`](https://stellar.expert/explorer/testnet/tx/4ac78a36c70f12025cf4706d711bb713efc4a1523ce6c29af5aea46f6c0c0d71) |
| 12 | freeze the cloud card | — | [`e1cb41851f23…`](https://stellar.expert/explorer/testnet/tx/e1cb41851f2377dc985ec8b7ccd54f45e81380007e154ca138a0446fb2f5141c) |
| 13 | authorize 2,400 on saas · aws invoice | — | [`24ff1688665b…`](https://stellar.expert/explorer/testnet/tx/24ff1688665b08ac1ee164b94bb630227012c0f3ccead229efda124c45913aa7) |
| 14 | declined · cloud card is frozen | **CardFrozen** | [`87366d3a93d7…`](https://stellar.expert/explorer/testnet/tx/87366d3a93d79676c737f797e781d61d5c62a3c5ddda75b211dc1da0a773046e) |
| 15 | authorize 37,000 on ai-tools | — | [`6ae14b371f83…`](https://stellar.expert/explorer/testnet/tx/6ae14b371f8312b159870ad62af4087fa2f458998a6975d15ae730e44268b6ab) |
| 16 | declined · 18,900 against 13,000 of limit left | **LimitExceeded** | [`333d000e1489…`](https://stellar.expert/explorer/testnet/tx/333d000e1489a3c4d8f0fcaa9a4d209225688df7efe5f583204abe6357929e50) |
| 17 | create card · misc, with a large limit | — | [`a3ba9e222663…`](https://stellar.expert/explorer/testnet/tx/a3ba9e22266386c68a007ccc0603ac1499f3425b136c72d5222837f13048470e) |
| 18 | allocate 100 to misc | — | [`7378c1c0be95…`](https://stellar.expert/explorer/testnet/tx/7378c1c0be953d90b30f2160c3025808f0678f99fd6dbdf27a15fde206115984) |
| 19 | declined · 5,000 against a card holding 100 | **InsufficientCardBalance** | [`e3d056d32ba2…`](https://stellar.expert/explorer/testnet/tx/e3d056d32ba24706483c6da0e07da715fb2bddbadeffd897b9235eb680e901f4) |
| 20 | refused · allocate past the unallocated balance | **6** | simulation |
| 21 | refused · allocate as somebody who holds no role | **3** | simulation |
<!-- END EVIDENCE-D2 -->

*Proves: the organisation created cards by category, allocated balances, set limits, and had spends approved or declined on the basis of available funds and the card's limit. Rows 14, 16 and 19 are three **different** declines with three different reasons, each a real transaction you can open.*

### Deliverable 3 · Withdrawal & Reconciliation

<!-- BEGIN EVIDENCE-D3 -->
| # | Step | Code | Transaction |
|---|---|---|---|
| 22 | whitelist the settlement address | — | [`7cfb5250d5b6…`](https://stellar.expert/explorer/testnet/tx/7cfb5250d5b64472fd9cb0dd934eda559d2a69f18a8f83dce4f978733ed9d4b6) |
| 23 | settle spend 1 · 2,400 USDC out to the settlement address | — | [`bb313936c369…`](https://stellar.expert/explorer/testnet/tx/bb313936c369aefcf90401ef0ac15b4d3016d1f81208a72fb72b68b8a5831ed3) |
| 24 | refused · settle the same spend twice | **12** | simulation |
| 25 | authorize 12,000 on advertising · google ads | — | [`fc27e62e1369…`](https://stellar.expert/explorer/testnet/tx/fc27e62e1369488c3ffa000eec96734300163de7b737f2e990c1658b820ee743) |
| 26 | refused · settle for more than the spend recorded | **11** | simulation |
| 27 | refused · settle as the admin rather than the operator | **3** | simulation |
| 28 | refused · settle to an address that is not whitelisted | **8** | simulation |
| 29 | settle spend 6 · 12,000 USDC out | — | [`ea79fd24be73…`](https://stellar.expert/explorer/testnet/tx/ea79fd24be7351f0f2f2aab1ba36f8ca8a8f05d317cb73442b6cbc6501f6fbeb) |
| 30 | indexer · rebuilt state from events, confirmed both settlements | — | off-chain |
<!-- END EVIDENCE-D3 -->

*Proves: an approved payment was settled by debiting the correct card, transferring stablecoin to an approved third party, and writing a reconciliation record on chain.*

### The reconciliation ledger

[**`smart-contract/.deploy/reconciliation.md`**](smart-contract/.deploy/reconciliation.md) — the same treasury rebuilt
**independently from its event stream** by `backend/indexer`, every figure in whole USDC, and
every settlement linked to the spend it paid.

It is regenerated by each `make demo` run, so it is not reproduced here: a table copied into this
file by hand is a table that goes stale the next time somebody runs the demo, and a stale
reconciliation ledger is worse than none.

The line to look for is **"2 of 2 settlements confirmed against the chain."** *Confirmed* means the
indexer fetched each transaction from RPC and found it succeeded — not that it took the contract's
word for it. A settlement the chain did not corroborate is a hard error that stops the run and
writes no file at all.

### Test results

```
smart-contract   128 tests passing · 96.5% line coverage over the contract
backend          18 tests passing · no network required
clippy            -D warnings, clean
```

[`smart-contract/.deploy/coverage.txt`](smart-contract/.deploy/coverage.txt) — a passing suite says nothing about what it never ran.

### How to check this yourself

1. Click any hash. Stellar Expert shows the transaction succeeded.
2. Click **row 14**, a decline. It **succeeded**, and the record says `CardFrozen`.
3. Click **row 23**, the settlement. 2,400 USDC left the contract.
4. Open the reconciliation table. Row `sp_1` links that same transaction to the spend the SaaS card authorised.
5. Or rebuild it yourself, from nothing but the chain:

```bash
cd backend
cargo run --release -- report --contract CA6IDMKW5QZKB6B5FFQSP3YPMXYOWHHRFGCFSBQS6Y7I25EB7D7VZZIY
```

No code needs to be read.

---

## What makes this evidence rather than a demo

**Every refusal in the run asserts its exact code**, and a mismatch stops the script and produces no file. Without that, a step failing for the *wrong* reason still passes: the script goes green, the evidence file fills up, and nobody has verified anything — which is worse than no evidence, because it looks like evidence.

**Refusals that never reach the ledger are labelled, never given a fabricated hash.** The CLI simulates before submitting, so a call the contract refuses produces no transaction. A fake hash is the one artefact a reviewer could not catch.

**The off-chain rebuild is asserted against the chain**, not trusted. An indexer that read nothing exits zero and writes a well-formed state file full of zeroes; the demo compares two independent views of the same treasury — the contract's view functions and the fold over its event stream — and fails if they disagree. Those assertions were added after exactly that bug went green during development.

---

## Repository layout

```
smart-contract/     the Soroban contract, 113 tests, and the deploy/demo scripts
backend/           the RPC indexer — module M16. A read-only CLI: no server, no database,
                    no keys. Not the application layer SOW §4.1 excludes — see docs/09
docs/               the specification, English (authoritative)
resources/flows/    one diagram per business flow, drawn from the shipped code
```

Two Cargo workspaces on purpose: the contract's `[profile.release]` — size optimisation and, critically, `overflow-checks = true` — only takes effect at a workspace root, and merging them would drop it with nothing failing to say so.

## Documentation

Start at [`docs/README.md`](docs/README.md). If you are checking the contract against the SOW, go straight to [`docs/13-deck-conformance.md`](docs/13-deck-conformance.md) — the deck line by line, naming the test that proves each line, and stating every place we deliberately built something else.

| | |
|---|---|
| The shape of an operation | [`resources/flows/`](resources/flows/README.md) |
| Every entry point | [`docs/02-contract-interface.md`](docs/02-contract-interface.md) |
| The frozen error and event tables | [`docs/08-events-and-errors.md`](docs/08-events-and-errors.md) |
| Invariants and the threat model | [`docs/07-invariants-and-security.md`](docs/07-invariants-and-security.md) |

## Make targets

```
make all        fmt, lint, test and build both trees
make test       every test in the repository
make coverage   line coverage over the contract
make deploy     deploy one treasury to testnet       KEY=<identity>
make demo       the acceptance scenario, end to end   KEY=<identity>
make verify     audit the committed evidence — no key, read-only
```

`make demo` writes `smart-contract/.deploy/evidence.md` and `smart-contract/.deploy/reconciliation.md`; `make coverage` writes
`smart-contract/.deploy/coverage.txt`. They are separate because only the first needs a key and a network.

## Out of scope

Per SOW §4.1, and nothing here pretends otherwise: frontend and backend application layer · card issuer or processor integration · stablecoin→fiat conversion · security audit · KYC/AML · multi-organisation tenancy · price oracle · **mainnet deployment**.
