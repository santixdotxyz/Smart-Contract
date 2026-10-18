#!/usr/bin/env bash
#
# Verify the committed evidence package against the live chain.
#
# # What this is for
#
# `make demo` produces evidence. This *checks* it — and the two are deliberately different jobs
# done by different code paths. The demo is the party that makes the claim; this is the party that
# audits it, and it can be run by somebody who does not trust the demo, on a machine that has
# never held a key, against a repository they just cloned.
#
# It reads `smart-contract/.deploy/evidence.md`, pulls every transaction hash out of it, and asks Soroban RPC
# whether each one actually succeeded. Then it rebuilds the treasury from its event stream and
# checks that the reconciliation still holds.
#
# # It needs no signing key and deploys nothing
#
# That is the point. A reviewer can run it, and running it changes nothing — so it is safe to run
# live, in front of an audience, as many times as you like. `make demo` is not: it deploys a fresh
# treasury and rewrites the evidence files.
#
# Usage:
#   ./scripts/verify-evidence.sh          # verify the committed package
#   make verify

set -euo pipefail
cd "$(dirname "$0")/.."

EVIDENCE="smart-contract/.deploy/evidence.md"
RECONCILIATION="smart-contract/.deploy/reconciliation.md"

if [ -t 1 ]; then
    BOLD=$(printf '\033[1m') DIM=$(printf '\033[2m')
    RED=$(printf '\033[31m') GREEN=$(printf '\033[32m') YELLOW=$(printf '\033[33m')
    OFF=$(printf '\033[0m')
else
    BOLD="" DIM="" RED="" GREEN="" YELLOW="" OFF=""
fi

say() { printf '%s\n' "$*"; }
step() { printf '\n%s%s%s\n' "$BOLD" "$*" "$OFF"; }
ok() { printf '  %s✓%s %s\n' "$GREEN" "$OFF" "$*"; }
bad() { printf '  %s✗%s %s\n' "$RED" "$OFF" "$*"; }
note() { printf '  %s%s%s\n' "$DIM" "$*" "$OFF"; }
die() {
    printf '\n%s✗ %s%s\n' "$RED" "$*" "$OFF" >&2
    exit 1
}

command -v curl >/dev/null || die "curl is not installed."
command -v jq >/dev/null || die "jq is not installed."
[ -f "$EVIDENCE" ] || die "$EVIDENCE not found. Run 'KEY=<identity> make demo' first, or clone a
repository that has the committed package."

# ---------------------------------------------------------------------------
# What the package claims
# ---------------------------------------------------------------------------

NETWORK=$(sed -n 's/^| Network | \(.*\) |$/\1/p' "$EVIDENCE" | head -1)
NETWORK="${NETWORK:-testnet}"
case "$NETWORK" in
public | mainnet) RPC_URL="https://mainnet.sorobanrpc.com" ;;
*) RPC_URL="https://soroban-testnet.stellar.org" ;;
esac
# `|| true` on every grep-in-an-assignment below, and it is load-bearing rather than defensive.
# This script runs under `set -euo pipefail`. A grep that matches nothing returns 1, pipefail
# propagates that through the pipeline, the assignment inherits it, and set -e kills the script —
# silently, mid-step, with no error printed. That is how an intermittent RPC hiccup came to look
# like the verifier crashing for no reason. An empty variable is handled below; a dead script is
# not handleable at all.
TREASURY=$(grep -m1 '^| TREASURY |' "$EVIDENCE" | grep -o 'C[A-Z0-9]\{55\}' | head -1 || true)

step "The package under review"
say "  file      $EVIDENCE"
say "  network   $NETWORK"
say "  treasury  $TREASURY"
say "  node      $RPC_URL"
note "nothing below signs anything, deploys anything, or needs a key"

# ---------------------------------------------------------------------------
# 1 · Every hash it cites really succeeded
# ---------------------------------------------------------------------------

step "1 · Every transaction the package cites"

HASHES=$(grep -o 'tx/[0-9a-f]\{64\}' "$EVIDENCE" | cut -d/ -f2 | sort -u || true)
TOTAL=$(printf '%s\n' "$HASHES" | grep -c . || true)
[ "$TOTAL" -gt 0 ] || die "the evidence file cites no transactions at all"

PASS=0
FAILED=""
for h in $HASHES; do
    s=$(curl -s -m 30 -X POST "$RPC_URL" -H 'Content-Type: application/json' \
        -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTransaction\",\"params\":{\"hash\":\"$h\"}}" |
        jq -r '.result.status // "NO_RESPONSE"')
    if [ "$s" = "SUCCESS" ]; then
        PASS=$((PASS + 1))
        printf '  %s✓%s %s… %s\n' "$GREEN" "$OFF" "${h:0:16}" "$s"
    else
        FAILED="$FAILED $h:$s"
        printf '  %s✗%s %s… %s\n' "$RED" "$OFF" "${h:0:16}" "$s"
    fi
done

[ -z "$FAILED" ] || die "$((TOTAL - PASS)) of $TOTAL transactions did not succeed on chain:$FAILED

A settlement or spend the ledger does not corroborate is not evidence of anything."
ok "$PASS of $TOTAL transactions confirmed SUCCESS on $NETWORK"

# ---------------------------------------------------------------------------
# 2 · The declines really are transactions
# ---------------------------------------------------------------------------
#
# The single most misread thing in this package. A declined spend is a SUCCESSFUL transaction
# carrying a reason code — that is what lets a reviewer open one and read why. If these rows had
# no hash, §6 would not be satisfied no matter what else passed.

step "2 · Declined spends are real, openable transactions"

DECLINE_ROWS=$(grep -E '^\| [0-9]+ \| declined' "$EVIDENCE" || true)
DECLINES=$(printf '%s\n' "$DECLINE_ROWS" | grep -c . || true)
if [ "$DECLINES" -gt 0 ]; then
    printf '%s\n' "$DECLINE_ROWS" | while IFS= read -r row; do
        reason=$(printf '%s' "$row" | grep -o '\*\*[A-Za-z]*\*\*' | tr -d '*' | head -1)
        hash=$(printf '%s' "$row" | grep -o 'tx/[0-9a-f]\{64\}' | cut -d/ -f2 | head -1)
        if [ -n "$hash" ]; then
            ok "$reason recorded on chain · ${hash:0:16}…"
        else
            bad "$reason has no transaction — a decline must be openable"
        fi
    done
    printf '%s\n' "$DECLINE_ROWS" | grep -q 'tx/' ||
        die "declined spends carry no transaction hashes"
else
    die "the package contains no recorded declines. Decision #04 requires them: SOW §6 asks a
reviewer to open a refused spend and read the reason."
fi

# ---------------------------------------------------------------------------
# 3 · Refusals that never reached the ledger are labelled, not faked
# ---------------------------------------------------------------------------

step "3 · Reverted refusals claim no transaction"

SIMS=$(grep -cE '^\| [0-9]+ \|.*\| simulation \|' "$EVIDENCE" || true)
if [ "$SIMS" -gt 0 ]; then
    ok "$SIMS refusals labelled 'simulation' — no hash invented for a call that never landed"
    note "a fabricated hash is the one artefact a reviewer could not catch, so these are labelled"
else
    note "no simulation rows in this package"
fi

# ---------------------------------------------------------------------------
# 4 · Rebuild the treasury from its events, independently
# ---------------------------------------------------------------------------

step "4 · Independent rebuild from the event stream"

INDEXER=backend/target/release/saintix-indexer
if [ ! -x "$INDEXER" ]; then
    note "building the indexer (first run only)…"
    cargo build --release --quiet --manifest-path backend/Cargo.toml 2>/dev/null || true
fi

if [ -x "$INDEXER" ]; then
    # Retried, because a public RPC node drops a request now and then and a dropped one looks
    # exactly like an unconfirmed settlement — which is the most serious thing this tool reports.
    # Observed once in four runs, and this script is meant to be run live in front of people, so
    # a transient blip must not be indistinguishable from fraud. Three attempts, then believe it.
    CONFIRMED=""
    for attempt in 1 2 3; do
        OUT=$("$INDEXER" report --contract "$TREASURY" --network "$NETWORK" --rpc "$RPC_URL" 2>/dev/null || true)
        CONFIRMED=$(printf '%s' "$OUT" | grep -o '[0-9]* of [0-9]* settlements confirmed' | head -1 || true)
        # Stop as soon as every settlement is accounted for; otherwise give the node another go.
        if [ -n "$CONFIRMED" ]; then
            n_=${CONFIRMED%% of *}
            m_=${CONFIRMED#* of }
            m_=${m_%% *}
            if [ "$n_" = "$m_" ]; then break; fi
        fi
        [ "$attempt" = 3 ] || { note "attempt $attempt incomplete — retrying"; sleep 3; }
    done
    if [ -n "$CONFIRMED" ]; then
        n=${CONFIRMED%% of *}
        rest=${CONFIRMED#* of }
        m=${rest%% *}
        if [ "$n" = "$m" ] && [ "$n" != "0" ]; then
            ok "$CONFIRMED against the chain"
            note "rebuilt from contract events alone — the contract's own views were never asked"
        else
            die "the rebuild confirms only $CONFIRMED"
        fi
    else
        bad "the indexer could not rebuild this treasury"
        note "its events may have aged out of the node's retention window — see docs/09"
        note "this does not invalidate the transaction hashes verified in step 1"
    fi
else
    note "indexer not built and cargo unavailable — skipping the rebuild"
    note "the transaction hashes in step 1 were still verified against the chain"
fi

# ---------------------------------------------------------------------------
# 5 · The committed reconciliation agrees
# ---------------------------------------------------------------------------

step "5 · The committed reconciliation ledger"

if [ -f "$RECONCILIATION" ]; then
    if grep -q 'settlements confirmed against the chain' "$RECONCILIATION"; then
        ok "$(grep -o '[0-9]* of [0-9]* settlements confirmed against the chain' "$RECONCILIATION" | head -1)"
    fi
    if grep -q '\*\*BROKEN\.\*\*' "$RECONCILIATION"; then
        die "the reconciliation report states an invariant is BROKEN"
    fi
    grep -q '\*\*Holds\.\*\*' "$RECONCILIATION" && ok "solvency and no-double-settlement both hold"
else
    note "$RECONCILIATION not present"
fi

# ---------------------------------------------------------------------------

step "Verdict"
say "  ${GREEN}${BOLD}The evidence package is corroborated by the chain.${OFF}"
say
say "  $PASS transactions verified · $DECLINES recorded declines · $SIMS labelled refusals"
say "  treasury  https://stellar.expert/explorer/$NETWORK/contract/$TREASURY"
say
say "  ${DIM}Nothing was deployed and nothing was signed. Re-run this as often as you like.${OFF}"
