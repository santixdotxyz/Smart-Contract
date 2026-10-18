#!/usr/bin/env bash
#
# Deploy one treasury for one organisation.
#
# # Deployment and construction happen in ONE transaction
#
# `stellar contract deploy -- <constructor args>` is the form that guarantees it. Deploying
# first and initialising second leaves a window in which anybody watching the ledger can call
# `initialize` and name themselves admin of a contract somebody else paid to deploy. The window
# is a few seconds wide and entirely sufficient.
#
# # The token
#
# By default this deploys `mock-token` as a stand-in USDC, because a self-contained demo cannot
# depend on the reader holding testnet USDC or establishing a trustline first. Point it at the
# real thing with:
#
#   TOKEN=CBIELTK6YBZJU5UP2WWQEUCYKLPU6AUNZ2BQ4WWFEIE3USCIHMXQDAMA ./scripts/deploy.sh
#
# The contract reads `decimals` from whatever it is given and never trusts the deployer, so both
# paths are the same code.
#
# Usage:
#   ./scripts/deploy.sh               on a test network, preflight generates and funds a signer
#   KEY=deployer ./scripts/deploy.sh  to sign with one of your own; required on public
#
# Writes $DEPLOY_DIR/env, which demo.sh sources.

set -euo pipefail
cd "$(dirname "$0")/.."
. scripts/lib.sh

preflight

TARGET=wasm32v1-none
TREASURY_WASM="target/$TARGET/release/saintix_treasury.wasm"
TOKEN_WASM="target/$TARGET/release/mock_token.wasm"

ADMIN_ADDR="${ADMIN_ADDR:-$(address_of "$KEY")}"
OPERATOR_ADDR="${OPERATOR_ADDR:-$ADMIN_ADDR}"

# ---------------------------------------------------------------------------

step "Building"
# --workspace, not just the root package. mock-token is only a dev-dependency of the treasury, so
# a plain `cargo build` never produces its wasm — and a default deploy needs it.
cargo build --workspace --target "$TARGET" --release >/dev/null 2>&1 ||
    die "cargo build failed; run it directly to see why"
ok "$(ls -lh "$TREASURY_WASM" | awk '{print $5}') treasury wasm"

# ---------------------------------------------------------------------------

# Where the contract's history begins, noted BEFORE anything is deployed.
#
# The indexer needs a ledger to start reading events from. Starting at the node's oldest retained
# one is correct and walks through a hundred thousand ledgers of unrelated traffic; starting here
# is the same answer in seconds. A few ledgers of margin, because the deploy lands slightly after
# this is read and an off-by-one here loses the `init` event.
DEPLOY_LEDGER=$(latest_ledger)
[ -n "$DEPLOY_LEDGER" ] || die "could not read the current ledger from $RPC_URL"
DEPLOY_LEDGER=$((DEPLOY_LEDGER > 5 ? DEPLOY_LEDGER - 5 : 1))

if [ -z "${TOKEN:-}" ]; then
    step "Deploying a stand-in USDC (7 decimals)"
    TOKEN=$(stellar contract deploy \
        --wasm "$TOKEN_WASM" --source "$KEY" --network "$NETWORK" \
        -- --decimals 7 2>/dev/null | tail -1)
    [ -n "$TOKEN" ] || die "token deployment produced no contract id"
    ok "$TOKEN"
else
    step "Using the token you named"
    ok "$TOKEN"
fi

# ---------------------------------------------------------------------------

step "Deploying the treasury"
note "admin    $ADMIN_ADDR"
note "operator $OPERATOR_ADDR"

TREASURY=$(stellar contract deploy \
    --wasm "$TREASURY_WASM" --source "$KEY" --network "$NETWORK" \
    -- \
    --admin "$ADMIN_ADDR" \
    --token_addr "$TOKEN" \
    --operators "[\"$OPERATOR_ADDR\"]" 2>/dev/null | tail -1)
[ -n "$TREASURY" ] || die "treasury deployment produced no contract id"
ok "$TREASURY"

# Read the configuration back off the chain rather than echoing what we passed in. A deploy that
# silently used different arguments than intended looks identical from here otherwise.
step "Verifying against the chain"
CONFIG=$(view "$TREASURY" get_config)
expect_eq "the configured token" "$(printf '%s' "$CONFIG" | jq -r .token)" "$TOKEN"
expect_eq "the founder" "$(printf '%s' "$CONFIG" | jq -r .founder)" "$ADMIN_ADDR"
ok "decimals $(printf '%s' "$CONFIG" | jq -r .decimals) · read from the token, not from us"

# ---------------------------------------------------------------------------

mkdir -p "$DEPLOY_DIR"
cat >"$DEPLOY_DIR/env" <<EOF
# Written by scripts/deploy.sh on $(date -u '+%Y-%m-%d %H:%M:%SZ'). Regenerated on every deploy.
export NETWORK="$NETWORK"
export RPC_URL="$RPC_URL"
export TREASURY="$TREASURY"
export TOKEN="$TOKEN"
export ADMIN="$ADMIN_ADDR"
export OPERATOR="$OPERATOR_ADDR"
export DEPLOY_LEDGER="$DEPLOY_LEDGER"
EOF

say
say "  treasury  $EXPLORER/contract/$TREASURY"
say "  token     $EXPLORER/contract/$TOKEN"
say "  env       $DEPLOY_DIR/env"
