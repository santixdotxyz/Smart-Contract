# 00 · Lifecycle

The whole thing, once, from an empty ledger to a reconciled payment.

```
DEPLOY          stellar contract deploy -- --admin --token_addr --operators
                one transaction. Deploy and construct together, or somebody
                else initialises it and names themselves admin
                    │
                    ▼
FUND            deposit(from, amount)                        anyone may
                total_deposited += amount
                    │
                    ▼
BUDGET          create_card(category, label, limit, window)   admin
                allocate(card_id, amount)                     admin
                    │                                          [6] over the pool
                    ▼
OPERATE         authorize_spend(card_id, amount, ref)         operator
                    │                                          ├─▶ Approved
                    │                                          └─▶ (R) Declined
                    │                                              + reason
                    ▼
PAY             settle(spend_id, to, amount)                  operator
                the card is debited · USDC leaves            [8] not whitelisted
                                                             [12] already settled
                    │
                    ▼
CHECK           backend/indexer verify                        anyone
                rebuild from events · fetch each settlement's
                transaction · confirm it succeeded
                    │
                    ▼
                Reconciled
```

## The two lanes, and why the contract is in the slow one

```
OFF-CHAIN · milliseconds
   swipe ──▶ gateway ──▶ decision ──▶ answer to the network
                 ╷            │
                 ╵ reads      │ writes, ≥ 1 ledger later
                 ╵            ▼
ON-CHAIN · ~5s per ledger
   contract state          authorize_spend          SpendRecord
   the rule book            the same 4 rules         + reason code
```

A card network wants an answer in about two seconds; a ledger closes in about five. The contract therefore cannot be in the path of a swipe, and does not try to be. It is where the rules are **defined** and where the decision is **recorded**.

Everything difficult in this design is the gap between the two lanes — see [06-authorize-spend.md](06-authorize-spend.md).

## Who may do what

```
                  admin        operator      anyone
deposit                                        ●
create_card         ●
allocate            ●                                 [3] otherwise
deallocate          ●
transfer_between    ●
set_limit           ●
set_card_status     ●            ●
authorize_spend                  ●                    [3] otherwise
add/remove_whitelist ●
settle                           ●                    [3] otherwise
views               ●            ●             ●
```

Two roles and no implicit promotion: an admin is not a lesser operator. They guard different things — the **shape** of the money versus its **movement**.

## What can never happen

```
a card holding money the treasury does not have      allocate refuses  [6]
money leaving to an address no admin approved        settle refuses    [8]
one spend paid twice                                 settle refuses   [12]
a declined spend being paid                          settle refuses   [10]
a contract with no admin                             revoke refuses   [13]
```
