# Saintix — reconciliation report

Rebuilt from the event stream of [`CBIB6RTTER6WGHFRAM5FIMKA6BCQZ27WV435Z3PQS2IXSKNHBK35B7FS`](https://stellar.expert/explorer/testnet/contract/CBIB6RTTER6WGHFRAM5FIMKA6BCQZ27WV435Z3PQS2IXSKNHBK35B7FS) on testnet.

State is folded from contract events alone — no database, no cached copy. Read up to ledger 4390000.

## Treasury

| | |
|---|---|
| Total deposited | 250,000 |
| Allocated to cards | 182,600 |
| Unallocated | 67,400 |
| Settled out | 14,400 |

## Cards

| # | Category | Label | Balance | Limit | Spent this window | Remaining | Status |
|---|---|---|---|---|---|---|---|
| 1 | SaaS | saas | 40,900 | 50,000 | 2,400 | 47,600 | Active |
| 2 | AI | ai-tools | 60,000 | 50,000 | 37,000 | 13,000 | Active |
| 3 | Advertising | advertising | 42,200 | 50,000 | 12,000 | 38,000 | Active |
| 4 | Cloud | cloud | 25,000 | 25,000 | 0 | 25,000 | Frozen |
| 5 | Other | misc | 100 | 90,000 | 0 | 90,000 | Active |

## Spend ledger

Declined rows are **transactions that succeeded**. The contract records a refusal rather than reverting, which is what lets you open one and read the reason.

| # | Card | Amount | Merchant | Decision | Reason | Settled | Transaction |
|---|---|---|---|---|---|---|---|
| 1 | saas | 2,400 | aws-invoice-0824 | Approved | — | yes | [`bb313936c369…`](https://stellar.expert/explorer/testnet/tx/bb313936c369aefcf90401ef0ac15b4d3016d1f81208a72fb72b68b8a5831ed3) |
| 2 | cloud | 500 | gcp-while-frozen | Declined | **CardFrozen** | no | [`87366d3a93d7…`](https://stellar.expert/explorer/testnet/tx/87366d3a93d79676c737f797e781d61d5c62a3c5ddda75b211dc1da0a773046e) |
| 3 | ai-tools | 37,000 | openai-sub | Approved | — | no | [`6ae14b371f83…`](https://stellar.expert/explorer/testnet/tx/6ae14b371f8312b159870ad62af4087fa2f458998a6975d15ae730e44268b6ab) |
| 4 | ai-tools | 18,900 | openai-sub-0824 | Declined | **LimitExceeded** | no | [`333d000e1489…`](https://stellar.expert/explorer/testnet/tx/333d000e1489a3c4d8f0fcaa9a4d209225688df7efe5f583204abe6357929e50) |
| 5 | misc | 5,000 | misc-oversized | Declined | **InsufficientCardBalance** | no | [`e3d056d32ba2…`](https://stellar.expert/explorer/testnet/tx/e3d056d32ba24706483c6da0e07da715fb2bddbadeffd897b9235eb680e901f4) |
| 6 | advertising | 12,000 | google-ads-0824 | Approved | — | yes | [`ea79fd24be73…`](https://stellar.expert/explorer/testnet/tx/ea79fd24be7351f0f2f2aab1ba36f8ca8a8f05d317cb73442b6cbc6501f6fbeb) |

## Reconciliation

2 of 2 settlements confirmed against the chain.

| # | Spend | Card | Amount | To | Transaction | Confirmed |
|---|---|---|---|---|---|---|
| 1 | sp_1 | saas | 2,400 | `GCM222FS…` | [`bb313936c369…`](https://stellar.expert/explorer/testnet/tx/bb313936c369aefcf90401ef0ac15b4d3016d1f81208a72fb72b68b8a5831ed3) | ✓ |
| 2 | sp_6 | advertising | 12,000 | `GCM222FS…` | [`ea79fd24be73…`](https://stellar.expert/explorer/testnet/tx/ea79fd24be7351f0f2f2aab1ba36f8ca8a8f05d317cb73442b6cbc6501f6fbeb) | ✓ |

## Deposits

| From | Amount | Ledger | Transaction |
|---|---|---|---|
| `GBCX2RVX…` | 250,000 | 4389955 | [`878ae3600061…`](https://stellar.expert/explorer/testnet/tx/878ae360006174fb21171f3cc0723484649b51994a8349f381c59c4128541675) |

## Invariants

- **Solvency** — Σ card.balance (168,200) + unallocated (67,400) = 235,600, and the treasury holds deposited − settled = 235,600. **Holds.**
- **No double settlement** — 0 spends settled more than once. **Holds.**
