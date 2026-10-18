# 02 · Deposit

```
anyone ──▶ deposit(from, amount)
                 │
       1. from.require_auth()                     ──▶ [3] Unauthorized
       2. amount > 0                              ──▶ [4] InvalidAmount
       3. Config exists                           ──▶ [2] NotInitialized
                 │
       4. total_deposited += amount
       5. assert I1
       6. emit deposit { from, amount, treasury }
                 │
       7. token.transfer(from ──▶ contract, amount)
                 │                        ▲
       8. bump TTL                        └── SECOND-TO-LAST, always
```

## The contract does not pull money

`from` is a **parameter**, not the invoker. The contract asks the token to move money the depositor has already authorised; it never reaches into an account.

That is why the signature is `deposit(from, amount)` and matches the deck exactly — this is the one entry point where no extra `caller` was needed.

## There is no token parameter

```
deposit(from, amount)
             ▲
             └── and nothing else. The token was fixed at construction.
```

"Only accepts the configured asset" is true **by construction** rather than by check. There is nothing to pass wrong.

## Anyone may deposit

No allowlist of depositors, and there does not need to be one. Money arriving is never the problem, and requiring a role would mean an organisation could not accept a transfer from its own second wallet without a governance step.

## Zero is refused

```
deposit(from, 0)   ──▶ [4] InvalidAmount
deposit(from, -1)  ──▶ [4] InvalidAmount
```

A zero-value deposit is a transaction that says nothing and a row the reconciliation table has to explain.

## What moves

```
before      total_deposited        0     unallocated        0     held        0
deposit 250,000
after       total_deposited  250,000     unallocated  250,000     held  250,000
                    ▲                            ▲                       ▲
              the counter               derived, not stored      the TOKEN's view
```

`unallocated = total_deposited − total_allocated`, so a deposit raises it without a second write. See [09-money-map.md](09-money-map.md).
