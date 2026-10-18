#!/usr/bin/env bash
#
# Shared plumbing for deploy.sh and demo.sh.
#
# Two functions matter: `call` and `call_decision`. Everything else is bookkeeping.

set -euo pipefail

NETWORK="${NETWORK:-testnet}"

# The RPC the scripts read from directly, for the things the CLI does not expose. Defaulted per
# network rather than required, so an ordinary testnet run needs no configuration.
if [ -z "${RPC_URL:-}" ]; then
    case "$NETWORK" in
    testnet) RPC_URL="https://soroban-testnet.stellar.org" ;;
    futurenet) RPC_URL="https://rpc-futurenet.stellar.org" ;;
    *) RPC_URL="" ;;
    esac
fi
export RPC_URL

# Working artefacts — the deploy env, the indexer's JSON dump, lcov. Regenerated on every run,
# never committed, and hidden because nobody needs to look at them.
DEPLOY_DIR="${DEPLOY_DIR:-.deploy}"

# The deliverable, and it lives beside the run that produced it.
#
# It shares `$DEPLOY_DIR` with the scratch, and the four files a reviewer reads are un-ignored by
# name in the root .gitignore while everything else there stays out. That is the settleflow layout.
#
# The cost is that `.deploy/` is a **hidden** directory — it does not appear in an IDE file tree,
# in Finder, or in `ls` without `-a`. SOW §6 asks that a Chapter Lead with minimal technical
# expertise open the repository and find the transaction hashes *without running anything*, and
# the last time these files lived here somebody looking for them could not find them. What stands
# in for browsing now is the root README, which links every one of them directly. Those links are
# load-bearing: breaking them makes the package unfindable rather than merely untidy.
#
# Resolved from this script's own location rather than from the working directory, so it lands in
# the same place however the script is invoked.
_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACT_ROOT="$(cd "$_LIB_DIR/.." && pwd)"
REPO_ROOT="$(cd "$_LIB_DIR/../.." && pwd)"
EVIDENCE_DIR="${EVIDENCE_DIR:-$CONTRACT_ROOT/$DEPLOY_DIR}"
EVIDENCE="$EVIDENCE_DIR/evidence.md"

case "$NETWORK" in
testnet) EXPLORER="https://stellar.expert/explorer/testnet" ;;
public) EXPLORER="https://stellar.expert/explorer/public" ;;
*) EXPLORER="" ;;
esac

# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

if [ -t 1 ]; then
    BOLD=$(printf '\033[1m')
    DIM=$(printf '\033[2m')
    RED=$(printf '\033[31m')
    GREEN=$(printf '\033[32m')
    OFF=$(printf '\033[0m')
else
    BOLD="" DIM="" RED="" GREEN="" OFF=""
fi

say() { printf '%s\n' "$*"; }
# Also records which deliverable the rows that follow belong to, so the evidence tables can be
# grouped without demo.sh repeating itself and without anyone maintaining a list of step numbers.
# The off-chain rebuild is D3's evidence: it is what turns a settlement record into a corroborated
# one, which is the whole of Deliverable 3's claim.
SECTION=""
step() {
    printf '\n%s%s%s\n' "$BOLD" "$*" "$OFF"
    case "$1" in
    "Deliverable 1"*) SECTION="D1" ;;
    "Deliverable 2"*) SECTION="D2" ;;
    "Deliverable 3"* | "Off-chain"*) SECTION="D3" ;;
    esac
}
ok() { printf '  %s✓%s %s\n' "$GREEN" "$OFF" "$*"; }
die() {
    printf '\n%s✗ %s%s\n' "$RED" "$*" "$OFF" >&2
    exit 1
}
note() { printf '  %s%s%s\n' "$DIM" "$*" "$OFF"; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is not installed. $2"
}

# The identity a test-network run falls back to when KEY is not set. An alias, never a secret:
# the CLI owns the file, the scripts only ever name it.
DEFAULT_KEY="${DEFAULT_KEY:-sx-deployer}"

# Create and fund the fallback deployer. Only ever reached on a network that has a friendbot —
# see the guard in preflight.
bootstrap_key() {
    if stellar keys address "$DEFAULT_KEY" >/dev/null 2>&1; then
        note "KEY not set — using the existing '$DEFAULT_KEY' identity on $NETWORK"
    else
        note "KEY not set — creating and funding '$DEFAULT_KEY' on $NETWORK"
        stellar keys generate --fund "$DEFAULT_KEY" --network "$NETWORK" >/dev/null 2>&1 ||
            die "could not create and fund '$DEFAULT_KEY' on $NETWORK.

Friendbot may be down, or this machine may have no writable identity directory. Create a signer
yourself and pass it:  KEY=<alias> make demo

See 'Providing the signing key' in README.md."
    fi
    # Friendbot is not always instant, and an alias left over from an earlier run may have been
    # spent down. Topping up is harmless — an already-funded account just returns an error this
    # run has no reason to care about.
    stellar keys fund "$DEFAULT_KEY" --network "$NETWORK" >/dev/null 2>&1 || true
    KEY="$DEFAULT_KEY"
    export KEY
}

preflight() {
    need stellar "Install the Stellar CLI: https://developers.stellar.org/docs/tools/cli"
    need jq "Install jq — the scripts parse contract output with it."
    need curl "Install curl — the scripts read the current ledger straight from RPC."
    # KEY may be either a stored identity ALIAS or a raw S… secret key. The Stellar CLI resolves
    # both, for `keys address` here and for `--source` in every call the scripts make, so the
    # scripts do not have to care which they were given — and a CI runner with no writable home
    # directory can pass the secret straight through the environment.
    #
    # With no KEY at all, a TEST network bootstraps one rather than stopping. demo.sh already
    # generates and funds the five role identities that way, and the deployer was the single
    # account a reviewer had to create by hand before anything would run — in a scenario whose
    # whole claim is "clone it, run one command". Friendbot XLM and a throwaway alias cost nothing
    # to mint and nothing to lose.
    #
    # `public` still refuses, and that asymmetry is the point: there is no friendbot there, and a
    # key generated behind somebody's back would own a real deployment while existing only in a
    # file nobody was told about.
    if [ -z "${KEY:-}" ]; then
        case "$NETWORK" in
        testnet | futurenet | local | standalone) bootstrap_key ;;
        *) die "KEY is not set, and '$NETWORK' is not a network this script will invent a signer
for. Nothing is generated automatically outside a test network.

It must be a funded ${NETWORK} signer, given as either:

  an identity alias   stellar keys generate deployer
                      export KEY=deployer

  a raw secret key    export KEY=S…            (56 characters, starts with S)
                      read -rs KEY             (safer: keeps it out of shell history)

See 'Providing the signing key' in README.md." ;;
        esac
    fi
    # A PUBLIC key passes the resolution check below — `keys address G…` simply echoes it back —
    # and then fails many steps later, inside the first deploy, with an error about the
    # transaction rather than about KEY. It is also the likeliest mistake anyone will make, since
    # a keypair has two halves and only one of them signs. Caught here, by shape, before any work.
    case "$KEY" in
    G*) die "KEY is a PUBLIC key (G…). It identifies an account but cannot sign for it.

Pass the secret half instead — 56 characters beginning with S — or the identity alias that
holds it:  stellar keys secret <alias>" ;;
    M*) die "KEY is a muxed address (M…), which cannot sign. Pass a secret key (S…) or an alias." ;;
    esac
    stellar keys address "$KEY" >/dev/null 2>&1 ||
        die "KEY is neither a known identity nor a usable secret key.

If it is an alias, create it:  stellar keys generate --fund $KEY --network $NETWORK
If it is a secret key, check it is the 56-character S… form."
    mkdir -p "$DEPLOY_DIR"
}

# ---------------------------------------------------------------------------
# The step recorder
# ---------------------------------------------------------------------------

STEP_NO=0
LAST_OUTPUT=""
EVIDENCE_ROWS=()

# Pull the transaction hash out of a CLI run.
#
# Parsed from the CLI's OWN label, not by scanning the output for 64 hex characters. Several
# things in this contract's arguments and output are long hex strings — an address in strkey form
# is not, but a contract id in hex is, and so is anything a future step passes as a raw key. A
# loose match picks one of those out of the echoed command line and records it as the transaction
# hash. An evidence file full of plausible-looking non-hashes would open to nothing on the
# explorer, and nobody would notice until a reviewer tried.
tx_hash_of() {
    printf '%s' "$1" | sed -n 's/.*Signing transaction: \([0-9a-f]\{64\}\).*/\1/p' | tail -1
}

# Invoke, and assert what the chain says.
#
#   call ""  <label> -- stellar contract invoke ...     expect success
#   call 6   <label> -- stellar contract invoke ...     expect exactly Error(Contract, #6)
#
# THE GREP IS THE WHOLE POINT. Without it a step that fails for the WRONG reason still passes,
# and the evidence package becomes a set of transactions nobody verified — which is worse than no
# evidence, because it looks like evidence.
#
# A refusal that never reaches the ledger is recorded as `simulation`, never given a fake hash.
# The CLI simulates before submitting, so a call the contract will refuse fails locally and
# produces no transaction at all. Inventing a hash for it would be fabricating the one artefact a
# reviewer is expected to open, and the only one they could not catch.
call() {
    local expect="$1"
    shift
    local label="$1"
    shift
    [ "$1" = "--" ] && shift

    STEP_NO=$((STEP_NO + 1))
    printf '  %2d. %-58s' "$STEP_NO" "$label"

    local out status
    set +e
    out=$("$@" 2>&1)
    status=$?
    set -e
    LAST_OUTPUT="$out"

    if [ $status -eq 0 ]; then
        if [ -n "$expect" ]; then
            printf '%sFAIL%s\n' "$RED" "$OFF"
            die "step $STEP_NO expected error $expect and the call SUCCEEDED.
A gate that should have refused did not. Output:
$out"
        fi
        local hash
        hash=$(tx_hash_of "$out")
        if [ -n "$hash" ]; then
            printf '%s✓%s %s%s…%s\n' "$GREEN" "$OFF" "$DIM" "${hash:0:16}" "$OFF"
            EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$label|—|\`$hash\`")
        else
            printf '%s✓%s\n' "$GREEN" "$OFF"
            EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$label|—|read-only")
        fi
        return 0
    fi

    if [ -z "$expect" ]; then
        printf '%sFAIL%s\n' "$RED" "$OFF"
        die "step $STEP_NO expected success and failed:
$out"
    fi

    # The assertion. `Error(Contract, #N)` is how the host reports a contract error, and matching
    # the exact number is what separates "it refused" from "it refused for the reason we claim".
    if printf '%s' "$out" | grep -q "Error(Contract, #$expect)"; then
        printf '%s✓%s %sblocked, code %s%s\n' "$GREEN" "$OFF" "$DIM" "$expect" "$OFF"
        EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$label|**$expect**|simulation")
        return 0
    fi

    printf '%sFAIL%s\n' "$RED" "$OFF"
    die "step $STEP_NO expected error $expect but got something else.
A step that fails for the wrong reason is not evidence of anything. Output:
$out"
}

# The wrinkle this contract has and collateral-vault does not: **a declined spend succeeds.**
#
# `authorize_spend` returns Ok and writes a record whether it approves or refuses, so a decline
# is a real transaction with a real hash — which is the whole point of decision #04, and what
# gives the evidence table a declined row a reviewer can open. `call` cannot check it, because
# there is no error to grep for.
#
#   call_decision Approved              <label> -- stellar contract invoke ...
#   call_decision Declined:LimitExceeded <label> -- stellar contract invoke ...
#
# The reason code is matched exactly, for the same reason the error number is: an approval that
# should have been declined, or a decline for the wrong reason, both mean the four checks are not
# running in the order the gateway assumes.
call_decision() {
    local want="$1"
    shift
    local label="$1"
    shift
    [ "$1" = "--" ] && shift

    local want_decision="${want%%:*}"
    local want_reason="${want#*:}"
    [ "$want_reason" = "$want" ] && want_reason=""

    STEP_NO=$((STEP_NO + 1))
    printf '  %2d. %-58s' "$STEP_NO" "$label"

    local out status
    set +e
    out=$("$@" 2>&1)
    status=$?
    set -e
    LAST_OUTPUT="$out"

    if [ $status -ne 0 ]; then
        printf '%sFAIL%s\n' "$RED" "$OFF"
        die "step $STEP_NO expected a recorded $want_decision and the call FAILED.
A decline is supposed to be a successful transaction — see decision #04. Output:
$out"
    fi

    # The CLI prints the return value as JSON on the last non-empty line. Everything else in the
    # capture is progress narration on stderr.
    local json decision reason
    json=$(printf '%s' "$out" | grep -o '{.*}' | tail -1)
    [ -n "$json" ] || die "step $STEP_NO returned no JSON to check. Output:
$out"
    decision=$(printf '%s' "$json" | jq -r '.decision // empty')
    reason=$(printf '%s' "$json" | jq -r '.reason_code // empty')

    if [ "$decision" != "$want_decision" ]; then
        printf '%sFAIL%s\n' "$RED" "$OFF"
        die "step $STEP_NO expected decision '$want_decision' and the contract said '$decision'
(reason '$reason'). The four checks are not producing the answer this scenario assumes."
    fi
    if [ -n "$want_reason" ] && [ "$reason" != "$want_reason" ]; then
        printf '%sFAIL%s\n' "$RED" "$OFF"
        die "step $STEP_NO was declined for '$reason', not '$want_reason'.
Declining for the wrong reason means the check ORDER has changed — see docs/05."
    fi

    local hash mark
    hash=$(tx_hash_of "$out")
    if [ "$want_decision" = "Declined" ]; then mark="**$reason**"; else mark="—"; fi
    if [ -n "$hash" ]; then
        printf '%s✓%s %s%s · %s…%s\n' "$GREEN" "$OFF" "$DIM" "${reason:-Approved}" "${hash:0:12}" "$OFF"
        EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$label|$mark|\`$hash\`")
    else
        printf '%s✓%s %s%s%s\n' "$GREEN" "$OFF" "$DIM" "${reason:-Approved}" "$OFF"
        EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$label|$mark|read-only")
    fi
}

# Record a step that is not a contract call at all — an off-chain check, an indexer run — so the
# evidence table shows it happened without pretending it produced a transaction.
record_offchain() {
    STEP_NO=$((STEP_NO + 1))
    printf '  %2d. %-58s%s✓%s %soff-chain%s\n' "$STEP_NO" "$1" "$GREEN" "$OFF" "$DIM" "$OFF"
    EVIDENCE_ROWS+=("$SECTION|$STEP_NO|$1|—|off-chain")
}

# ---------------------------------------------------------------------------
# Convenience wrappers
# ---------------------------------------------------------------------------

# Fire and forget, quietly. The CLI narrates every submission on stderr, which is useful when a
# human runs it and pure noise inside a scripted scenario — it buries the step lines the reader
# is actually following.
invoke() {
    local id="$1"
    shift
    local as="${AS:-$KEY}"
    stellar contract invoke --id "$id" --source "$as" --network "$NETWORK" -- "$@" 2>/dev/null
}

# A read-only call whose output is JSON. Not routed through `call`, because a view emits no
# transaction and recording it as a step would pad the evidence with rows nobody can open.
view() {
    local id="$1"
    shift
    stellar contract invoke --id "$id" --source "$KEY" --network "$NETWORK" -- "$@" 2>/dev/null
}

address_of() { stellar keys address "$1"; }

# The ledger the network is on right now.
#
# Recorded at deploy time and handed to the indexer as `--from-ledger`. Without it the indexer
# starts at the node's oldest retained ledger and walks forward through a hundred thousand
# ledgers of other people's traffic to find a contract that was deployed a minute ago.
latest_ledger() {
    curl -s -m 20 -X POST "$RPC_URL" -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","id":1,"method":"getLatestLedger"}' |
        jq -r '.result.sequence // empty'
}

# Assert a view returns what the scenario assumes, BEFORE acting on it.
#
# Without this a mistaken figure surfaces three steps later as a confusing error from a call that
# was not the problem. Named after the thing being checked so the failure says what is wrong.
expect_eq() {
    local what="$1" got="$2" want="$3"
    [ "$got" = "$want" ] || die "$what is $got, and this scenario needs $want.
The figures in demo.sh are derived from each other; changing one means re-deriving the rest."
}

# ---------------------------------------------------------------------------
# Evidence
# ---------------------------------------------------------------------------

write_evidence() {
    local title="$1"
    mkdir -p "$EVIDENCE_DIR"
    {
        echo "# $title"
        echo
        echo "Generated by \`make demo\` on $(date -u '+%Y-%m-%d %H:%M:%SZ')."
        echo
        echo "**How to check this.** Open each transaction hash on Stellar Expert and confirm it"
        echo "succeeded."
        echo
        echo "Three kinds of row appear, and the difference is the point:"
        echo
        echo "| Row | What it means |"
        echo "|---|---|"
        echo "| a hash, no error code | The call succeeded. |"
        echo "| a hash **and** an error code in bold | The spend was **declined and recorded**. A decline is a successful transaction here — that is what lets you open it and read the reason. |"
        echo "| \`simulation\` | The contract refused and the call never reached the ledger. The CLI simulates before submitting, so there is no transaction to link. These rows are labelled rather than given a hash, because a fabricated hash is the one artefact a reviewer would have no way to catch. |"
        echo
        echo "Every refusal below was matched against its **exact** error or reason code. A step"
        echo "that failed for the wrong reason would have stopped this run and produced no file."
        echo
        echo "| | |"
        echo "|---|---|"
        echo "| Network | $NETWORK |"
        for var in TREASURY TOKEN; do
            [ -n "${!var:-}" ] && echo "| $var | [\`${!var}\`]($EXPLORER/contract/${!var}) |"
        done
        for var in ADMIN OPERATOR SETTLEMENT ROGUE OUTSIDER; do
            [ -n "${!var:-}" ] && echo "| $var | \`${!var}\` |"
        done
        echo
        echo "| # | Step | Code | Transaction |"
        echo "|---|---|---|---|"
        render_rows
        echo
    } >"$EVIDENCE"
}

# ---------------------------------------------------------------------------
# One renderer, two destinations
# ---------------------------------------------------------------------------

# Emit EVIDENCE_ROWS as markdown table rows. With an argument, only the rows belonging to that
# deliverable — the tag `step` recorded when the section began.
#
# Shared by `write_evidence` and `sync_readme` on purpose: the README's table and the evidence
# file's table are the same claim, and two renderers would eventually disagree about it.
render_rows() {
    local want="${1:-}"
    printf '%s\n' "${EVIDENCE_ROWS[@]}" |
        awk -F'|' -v ex="$EXPLORER" -v want="$want" '
            want != "" && $1 != want { next }
            {
                tx = $5
                if (tx ~ /^`[0-9a-f]+`$/) {
                    h = substr(tx, 2, length(tx) - 2)
                    tx = "[`" substr(h, 1, 12) "…`](" ex "/tx/" h ")"
                }
                printf "| %s | %s | %s | %s |\n", $2, $3, $4, tx
            }'
}

# Replace the content between <!-- BEGIN name --> and <!-- END name --> in a file.
splice() {
    local file="$1" name="$2" content="$3"
    awk -v n="$name" -v f="$content" '
        $0 ~ "<!-- BEGIN " n " -->" { print; while ((getline l < f) > 0) print l; skip = 1; next }
        $0 ~ "<!-- END " n " -->"   { skip = 0 }
        !skip { print }
    ' "$file" >"$file.tmp" && mv "$file.tmp" "$file"
}

# Rewrite the README's evidence tables from the run that just happened.
#
# The deck asks for the table to live in the README — *"a single table in the README, each row a
# step with a hash linking to Stellar Expert, so the Chapter Lead ticks §6.2 without reading
# code"*. Keeping it there by hand means every `make demo` silently leaves it describing a
# treasury that no longer exists, which is worse than not having it. So the run writes it.
sync_readme() {
    local readme="$REPO_ROOT/README.md" tmp
    [ -f "$readme" ] || return 0
    grep -q "<!-- BEGIN EVIDENCE-D1 -->" "$readme" || return 0
    tmp=$(mktemp -d)

    {
        echo "| | |"
        echo "|---|---|"
        echo "| Network | $NETWORK |"
        echo "| Treasury contract | [\`$TREASURY\`]($EXPLORER/contract/$TREASURY) |"
        echo "| Token (stand-in USDC, 7 decimals) | [\`$TOKEN\`]($EXPLORER/contract/$TOKEN) |"
        echo "| Admin | \`$ADMIN\` |"
        echo "| Operator | \`$OPERATOR\` |"
        echo "| Settlement address (whitelisted) | \`$SETTLEMENT\` |"
    } >"$tmp/hdr"
    splice "$readme" "EVIDENCE-HEADER" "$tmp/hdr"

    local d
    for d in D1 D2 D3; do
        {
            echo "| # | Step | Code | Transaction |"
            echo "|---|---|---|---|"
            render_rows "$d"
        } >"$tmp/$d"
        splice "$readme" "EVIDENCE-$d" "$tmp/$d"
    done
    rm -rf "$tmp"
    ok "README evidence tables rewritten from this run"
}
