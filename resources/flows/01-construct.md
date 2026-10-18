# 01 · Construct

```
deployer ──▶ stellar contract deploy --wasm … -- \
                 --admin G… --token_addr C… --operators '["G…"]'
                              │
                              ▼
                       __constructor
                              │
                   1. Config already exists?  ──▶ [1] AlreadyInitialized
                              │ no
                   2. token.decimals()        - -▶ the TOKEN contract
                              │                    never the deployer's claim
                   3. write Config { founder, token, decimals }
                   4. write Treasury { 0, 0, 0 }
                   5. Roles[admin] = Admin
                      Roles[op]    = Operator   for each operator
                   6. emit init
                   7. bump instance TTL
```

## One transaction, not two

Deploying and then initialising leaves a window — a few seconds, entirely sufficient — in which anybody watching the ledger can call `initialize` and name themselves admin of a contract somebody else paid to deploy.

`stellar contract deploy -- <constructor args>` is the form that closes it, and it is the only form `deploy.sh` uses.

## `initialize` exists and always refuses

```
anyone ──▶ initialize(admin, token, operators) ──▶ [1] AlreadyInitialized
```

Because `__constructor` runs on every deployment, `initialize` can only ever be the second call. That is the point rather than an accident: the deck's API surface lists `initialize` and asks that a second call be refused; the constructor is what removes the window in which somebody else could make the *first* one.

## Decimals come from the token

```
     Config.decimals  ◀- - -  token.decimals()
              ▲
              └─── NOT from the deployer
```

It is used in no arithmetic — there is none in this contract that scales. It is what an explorer and the indexer render amounts with, and a value copied from an assumption rather than read from the token would be wrong by orders of magnitude with nothing on chain to contradict it.

## The founder holds nothing

```
Config.founder     a record of who deployed
Roles[founder]     the actual privilege — and revocable
```

The founder is seeded as an ordinary admin and can be revoked like any other, **provided somebody else holds `Admin` by then**. See [00-lifecycle.md](00-lifecycle.md) for the last-admin guard.

## One instance per organisation

No factory. No registry. No shared state. Two treasuries deployed from the same wasm know nothing about each other — card ids restart at 1 in each, and neither can read the other's anything.

Multi-organisation tenancy is out of scope, and this is what "out of scope" means concretely.
