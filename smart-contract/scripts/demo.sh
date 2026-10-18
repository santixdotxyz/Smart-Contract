#!/usr/bin/env bash
#
# The acceptance scenario. One command, on a clean machine, end to end.
#
# # What this is for
#
# SOW §6 asks for evidence a Chapter Lead can review *with minimal technical expertise*. This
# deploys a treasury, drives the full workflow through it — deposit → cards → allocate →
# authorize → withdraw → reconcile — and writes `smart-contract/.deploy/evidence.md` with a transaction hash for
# every step that reached the ledger.
#
# # The property that makes it evidence rather than theatre
#
# **Every refusal asserts its exact code and fails the script on a mismatch.** Without that, a
# step that fails for the *wrong* reason still passes — the script goes green, the evidence file
# fills up, and nobody has verified anything. That is worse than no evidence, because it looks
# like evidence. See `call` and `call_decision` in scripts/lib.sh.
#
# # Two kinds of refusal, and they look different on chain
#
# This contract has both, and the distinction is the single most important thing in the file:
#
#   * A **declined spend is a successful transaction.** `authorize_spend` writes a record saying
#     what was refused and why (decision #04), so it has a real hash a reviewer can open. Those
#     steps use `call_decision`.
#   * A **refused call reverts.** The CLI simulates before submitting, so it never reaches the
#     ledger and there is no transaction. Those rows are labelled `simulation`, never given a
#     fabricated hash. Those steps use `call` with an expected error number.
#
# Eight refusals across eight distinct codes: 3, 6, 8, 11, 12, and reason codes CardFrozen,
# LimitExceeded, InsufficientCardBalance.
#
# # This scenario also exists as a test
#
# `src/test/demo_replay.rs` is the same sequence in process. It runs in under a second and it is
# where a broken scenario should fail — not here, after several minutes of funded transactions.
# Keep the two in step.
#
# Usage:
#   ./scripts/demo.sh                 on a test network, preflight generates and funds a signer
#   KEY=deployer ./scripts/demo.sh    to sign with one of your own; required on public

set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/lib.sh

preflight

# ---------------------------------------------------------------------------
# Cast
# ---------------------------------------------------------------------------

step "Accounts"

# Five identities, because every guard needs somebody who is *not* allowed to pass it. Sharing
# one would let a check pass because the caller happened to be authorised for something else,
# which is exactly the confusion the roles exist to prevent.
for role in admin operator outsider settlement rogue; do
    id="sx_$role"
    if ! stellar keys address "$id" >/dev/null 2>&1; then
        stellar keys generate --fund "$id" --network "$NETWORK" >/dev/null 2>&1 ||
            die "could not create and fund '$id'"
    fi
    # Friendbot is not always instant, and an unfunded source produces an error that looks
    # nothing like the real problem.
    stellar keys fund "$id" --network "$NETWORK" >/dev/null 2>&1 || true
done

ADMIN=$(address_of sx_admin)
OPERATOR=$(address_of sx_operator)
OUTSIDER=$(address_of sx_outsider)   # holds no role at all
SETTLEMENT=$(address_of sx_settlement) # the whitelisted destination
ROGUE=$(address_of sx_rogue)         # a destination that is deliberately never whitelisted
ok "admin · operator · outsider · settlement · rogue"

# ---------------------------------------------------------------------------
# Deploy
# ---------------------------------------------------------------------------

step "Deploying a fresh treasury"
ADMIN_ADDR="$ADMIN" OPERATOR_ADDR="$OPERATOR" ./scripts/deploy.sh >/dev/null ||
    die "deploy.sh failed; run it directly to see why"
# shellcheck disable=SC1090,SC1091
. "$DEPLOY_DIR/env"
ADMIN="$ADMIN" # re-export after sourcing, which overwrites it with the same value
ok "treasury $TREASURY"

# ---------------------------------------------------------------------------
# Scale
# ---------------------------------------------------------------------------
#
# USDC has 7 decimals, so one whole unit is 10,000,000 of the smallest. Every figure below is
# written as a multiple of USD rather than as a raw integer, because a misplaced zero in a
# fourteen-digit literal is invisible and a misplaced zero here is not.
#
# The figures are the deck's own, from the Treasury Overview and Cards Grid wireframes. They are
# derived from each other — 250,000 deposited, 182,500 allocated, 67,500 unallocated — so
# changing one means re-deriving the rest. The script asserts them against the CHAIN before it
# acts on them.

USD=10000000

DEPOSIT=$((250000 * USD))
A_SAAS=$((42300 * USD))
A_AI=$((61000 * USD))
A_ADS=$((58400 * USD))
A_CLOUD=$((25000 * USD))
DEALLOC=$((4200 * USD)) # ads: 58,400 → 54,200, the wireframe's figure
REBALANCE=$((1000 * USD))

LIMIT_STD=$((50000 * USD))
LIMIT_CLOUD=$((25000 * USD))

SPEND_OK=$((2400 * USD))     # aws invoice — the one that settles
SPEND_AI_OK=$((37000 * USD)) # takes the AI card to 37,000 of its 50,000 limit
SPEND_AI_NO=$((18900 * USD)) # …so 18,900 is refused against the 13,000 that is left
SPEND_ADS=$((12000 * USD))
SPEND_FROZEN=$((500 * USD))
SPEND_BROKE=$((5000 * USD))
MISC_ALLOC=$((100 * USD))

MINT=$((1000000 * USD))

as_admin() { AS=sx_admin "$@"; }

# ---------------------------------------------------------------------------
# D1 · Treasury
# ---------------------------------------------------------------------------

step "Deliverable 1 · On-chain treasury"

# Fund the admin with the stand-in token so there is something to deposit. Against real USDC this
# step is a faucet and a trustline instead, and nothing else in the scenario changes.
if [ -z "${TOKEN_IS_REAL:-}" ]; then
    invoke "$TOKEN" mint --to "$ADMIN" --amount "$MINT" >/dev/null
fi

call "" "deposit 250,000 USDC into the treasury" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    deposit --from "$ADMIN" --amount "$DEPOSIT"

T=$(view "$TREASURY" get_treasury)
expect_eq "total_deposited" "$(printf '%s' "$T" | jq -r .total_deposited)" "$DEPOSIT"
expect_eq "unallocated" "$(view "$TREASURY" get_unallocated | tr -d '"')" "$DEPOSIT"
ok "the chain agrees: 250,000 in, 250,000 unallocated"

# ---------------------------------------------------------------------------
# D2 · Cards
# ---------------------------------------------------------------------------

step "Deliverable 2 · Category cards"

for spec in "SaaS:saas:$LIMIT_STD" "AI:ai-tools:$LIMIT_STD" \
    "Advertising:advertising:$LIMIT_STD" "Cloud:cloud:$LIMIT_CLOUD"; do
    IFS=: read -r cat label limit <<<"$spec"
    call "" "create card · $label" -- \
        stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
        create_card --caller "$ADMIN" --category "\"$cat\"" --label "$label" --limit "$limit" --window_secs 0
done

SAAS=1
AI=2
ADS=3
CLOUD=4

call "" "allocate 42,300 to saas" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $SAAS --amount "$A_SAAS"
call "" "allocate 61,000 to ai-tools" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $AI --amount "$A_AI"
call "" "allocate 58,400 to advertising" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $ADS --amount "$A_ADS"
call "" "allocate 25,000 to cloud" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $CLOUD --amount "$A_CLOUD"

# Both ways money moves without leaving the contract. The SOW asks for a transaction proving each.
call "" "deallocate 4,200 from advertising back to the pool" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    deallocate --caller "$ADMIN" --card_id $ADS --amount "$DEALLOC"
call "" "rebalance 1,000 from ai-tools to saas" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    transfer_between_cards --caller "$ADMIN" --from_id $AI --to_id $SAAS --amount "$REBALANCE"

expect_eq "unallocated after allocation" \
    "$(view "$TREASURY" get_unallocated | tr -d '"')" "$((67500 * USD))"
ok "182,500 allocated · 67,500 unallocated — the wireframe's figures, off the chain"

call "" "freeze the cloud card" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    set_card_status --caller "$ADMIN" --card_id $CLOUD --status '"Frozen"'

# ---------------------------------------------------------------------------
# D2 · Spending control
# ---------------------------------------------------------------------------

step "Deliverable 2 · Spending control"

# The one that settles later.
call_decision Approved "authorize 2,400 on saas · aws invoice" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $SAAS --amount "$SPEND_OK" \
    --merchant_ref aws-invoice-0824

SPEND_OK_ID=$(printf '%s' "$LAST_OUTPUT" | grep -o '{.*}' | tail -1 | jq -r .id)

# Three declines, three reasons, three real transactions. This is decision #04 on chain: each of
# these has a hash, and opening it shows why the spend was refused.
call_decision Declined:CardFrozen "declined · cloud card is frozen" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $CLOUD --amount "$SPEND_FROZEN" \
    --merchant_ref gcp-while-frozen

call_decision Approved "authorize 37,000 on ai-tools" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $AI --amount "$SPEND_AI_OK" \
    --merchant_ref openai-sub

expect_eq "remaining limit on ai-tools" \
    "$(view "$TREASURY" get_remaining_limit --card_id $AI | tr -d '"')" "$((13000 * USD))"

call_decision Declined:LimitExceeded "declined · 18,900 against 13,000 of limit left" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $AI --amount "$SPEND_AI_NO" \
    --merchant_ref openai-sub-0824

# A card with limit to spare and no money, so the decline can only come from check 3.
call "" "create card · misc, with a large limit" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    create_card --caller "$ADMIN" --category '"Other"' --label misc --limit "$((90000 * USD))" --window_secs 0
MISC=5
call "" "allocate 100 to misc" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $MISC --amount "$MISC_ALLOC"

call_decision Declined:InsufficientCardBalance "declined · 5,000 against a card holding 100" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $MISC --amount "$SPEND_BROKE" \
    --merchant_ref misc-oversized

# Two refusals that revert. No transaction, so no hash — see the note at the top.
call 6 "refused · allocate past the unallocated balance" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    allocate --caller "$ADMIN" --card_id $SAAS --amount "$((500000 * USD))"

call 3 "refused · allocate as somebody who holds no role" -- \
    stellar contract invoke --id "$TREASURY" --source sx_outsider --network "$NETWORK" -- \
    allocate --caller "$OUTSIDER" --card_id $SAAS --amount "$USD"

# ---------------------------------------------------------------------------
# D3 · Withdrawal and reconciliation
# ---------------------------------------------------------------------------

step "Deliverable 3 · Withdrawal & reconciliation"

call "" "whitelist the settlement address" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    add_whitelist --caller "$ADMIN" --who "$SETTLEMENT"

# The only step where money leaves the contract.
call "" "settle spend $SPEND_OK_ID · 2,400 USDC out to the settlement address" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    settle --caller "$OPERATOR" --spend_id "$SPEND_OK_ID" --to "$SETTLEMENT" --amount "$SPEND_OK"

expect_eq "the settlement address balance" \
    "$(view "$TOKEN" balance --id "$SETTLEMENT" | tr -d '"')" "$SPEND_OK"
ok "2,400 USDC arrived, and the saas card paid for it"

call 12 "refused · settle the same spend twice" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    settle --caller "$OPERATOR" --spend_id "$SPEND_OK_ID" --to "$SETTLEMENT" --amount "$SPEND_OK"

call_decision Approved "authorize 12,000 on advertising · google ads" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    authorize_spend --caller "$OPERATOR" --card_id $ADS --amount "$SPEND_ADS" \
    --merchant_ref google-ads-0824
SPEND_ADS_ID=$(printf '%s' "$LAST_OUTPUT" | grep -o '{.*}' | tail -1 | jq -r .id)

call 11 "refused · settle for more than the spend recorded" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    settle --caller "$OPERATOR" --spend_id "$SPEND_ADS_ID" --to "$SETTLEMENT" \
    --amount "$((SPEND_ADS + USD))"

call 3 "refused · settle as the admin rather than the operator" -- \
    stellar contract invoke --id "$TREASURY" --source sx_admin --network "$NETWORK" -- \
    settle --caller "$ADMIN" --spend_id "$SPEND_ADS_ID" --to "$SETTLEMENT" --amount "$SPEND_ADS"

call 8 "refused · settle to an address that is not whitelisted" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    settle --caller "$OPERATOR" --spend_id "$SPEND_ADS_ID" --to "$ROGUE" --amount "$SPEND_ADS"

call "" "settle spend $SPEND_ADS_ID · 12,000 USDC out" -- \
    stellar contract invoke --id "$TREASURY" --source sx_operator --network "$NETWORK" -- \
    settle --caller "$OPERATOR" --spend_id "$SPEND_ADS_ID" --to "$SETTLEMENT" --amount "$SPEND_ADS"

# ---------------------------------------------------------------------------
# Off-chain verification
# ---------------------------------------------------------------------------

step "Off-chain · rebuild and reconcile"

INDEXER=../backend/target/release/saintix-indexer
[ -x "$INDEXER" ] ||
    cargo build --release --manifest-path ../backend/Cargo.toml >/dev/null 2>&1 ||
    die "could not build the indexer, and M16 is not optional — this run would produce an
evidence file with no off-chain verification in it."

# `verify` rebuilds the treasury from its events and then fetches each settlement's transaction
# from RPC to confirm it succeeded. A settlement record the chain does not corroborate is exactly
# what a reconciliation ledger exists to surface.
"$INDEXER" verify --contract "$TREASURY" --rpc "$RPC_URL" --network "$NETWORK" \
    --from-ledger "$DEPLOY_LEDGER" --out "$DEPLOY_DIR/index.json" ||
    die "the indexer could not reconcile the settlements it just watched happen"

# THE ASSERTION THAT MAKES THE STEP ABOVE MEAN SOMETHING.
#
# An indexer that read nothing exits zero and writes a perfectly well-formed state file full of
# zeroes. Without these four checks, a paging bug in the indexer looks identical to a successful
# reconciliation — the script goes green, `evidence.md` gets an "indexer · verified" row, and the
# one artefact that was supposed to independently corroborate the chain corroborates nothing.
# That happened during development, which is why the checks are here.
#
# These compare two INDEPENDENT views of the same treasury: what the contract's view functions
# say, and what the indexer folded out of the event stream. Agreement between them is the whole
# claim module M13 makes.
IDX="$DEPLOY_DIR/index.json"
expect_eq "settlements the indexer rebuilt" "$(jq '.settlements | length' "$IDX")" "2"
expect_eq "settlements it confirmed against the chain" \
    "$(jq '[.settlements[] | select(.reconciled)] | length' "$IDX")" "2"
expect_eq "spend records the indexer rebuilt" "$(jq '.spends | length' "$IDX")" "6"
expect_eq "declines the indexer rebuilt" \
    "$(jq '[.spends[] | select(.decision == "Declined")] | length' "$IDX")" "3"
expect_eq "cards the indexer rebuilt" "$(jq '.cards | length' "$IDX")" "5"
expect_eq "total_settled, rebuilt from events alone" \
    "$(jq -r '.treasury.total_settled' "$IDX")" "$((14400 * USD))"
ok "the rebuild agrees with the chain on all five cards, six spends and both settlements"

record_offchain "indexer · rebuilt state from events, confirmed both settlements"

"$INDEXER" report --contract "$TREASURY" --rpc "$RPC_URL" --network "$NETWORK" \
    --from-ledger "$DEPLOY_LEDGER" >"$EVIDENCE_DIR/reconciliation.md"
grep -q "2 of 2 settlements confirmed" "$EVIDENCE_DIR/reconciliation.md" ||
    die "the reconciliation report does not say both settlements were confirmed"
ok "reconciliation table → smart-contract/.deploy/reconciliation.md"

# ---------------------------------------------------------------------------
# The ledger, read back off the chain
# ---------------------------------------------------------------------------

step "The reconciliation ledger"

view "$TREASURY" list_settlements | jq -r '
    "  " + (.[] | "settlement \(.id)  spend \(.spend_id)  card \(.card_id)  \(.amount)")'

FINAL=$(view "$TREASURY" get_treasury)
expect_eq "total_settled" "$(printf '%s' "$FINAL" | jq -r .total_settled)" "$((14400 * USD))"
expect_eq "the settlement address balance" \
    "$(view "$TOKEN" balance --id "$SETTLEMENT" | tr -d '"')" "$((14400 * USD))"

DECLINES=$(view "$TREASURY" list_spends --card_id 0 | jq '[.[] | select(.decision == "Declined")] | length')
expect_eq "recorded declines" "$DECLINES" "3"
ok "6 spend records · 3 approved · 3 declined and recorded on chain"

# ---------------------------------------------------------------------------

write_evidence "Saintix SOW 1 — acceptance evidence"
sync_readme

step "Done"
say "  smart-contract/.deploy/evidence.md        a transaction hash per step"
say "  smart-contract/.deploy/reconciliation.md  the ledger, rebuilt independently from events"
say
say "  treasury  $EXPLORER/contract/$TREASURY"
say
say "  The third file of the package, smart-contract/.deploy/coverage.txt, comes from 'make coverage' —"
say "  it needs no key and no network, so it is a separate command."
say
say "  Every refusal above was matched against its exact code. If any step had failed for the"
say "  wrong reason, this script would have stopped and written nothing."
