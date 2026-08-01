# scarce-studio

Converts pay.sh catalog misses into funded builds: a failed catalog search is
a willingness-to-pay signal, the studio builds against it, and the deliverable
is a gated pay.sh endpoint the buyer's own agent then consumes.

Read in order: [DESIGN.md](DESIGN.md) (why it works),
[ARCHITECTURE.md](ARCHITECTURE.md) (system shape),
[PLAN.md](PLAN.md) (milestones M0→M6).

**Invariant:** the API holds no authoritative state. Every fact it serves is a
projection of signed Nostr events or on-chain state; every state transition
cites its evidence (event id or tx signature). The SQLite database is
droppable and rebuildable from the substrates.

## Layout

One binary — `scarced` (axum HTTP surface + orchestrator loop) — over five
crates:

| Crate | Role |
|---|---|
| `studio-core` | domain types, schemas, state machine + gate engine (no I/O) |
| `studio-store` | SQLite projections (rebuildable by design) |
| `studio-buzz` | `BuzzPort`: Buzz-crate-backed impl + mock (M3) |
| `studio-pay` | `PayPort`: stub impl through M4, live MPP session impl in M5 |
| `studio-api` | axum routes, auth, SSE |

Implemented so far: RFQ capture (M1) and quote issuance + gate-policy engine
(M2). The orchestrator loop and Buzz integration arrive in M3.

## Install

```bash
just install scarce            # cargo-installs the `scarced` binary
```

## Run

```bash
cp scarced.example.yaml scarced.yaml       # points at wss://scarce.communities.buzz.xyz
scarced --config scarced.yaml              # or `just run --config scarced.yaml`
```

Config precedence: defaults ← YAML ← `SCARCED_*` env (figment). Nested keys
join with `__` in env form. `--config` is optional — env-only also works:

```bash
SCARCED_STUDIO_TOKEN=dev-token scarced
```

| Key | Env | Default | |
|---|---|---|---|
| `bind` | `SCARCED_BIND` | `127.0.0.1:7380` | HTTP bind address |
| `db` | `SCARCED_DB` | `sqlite://scarced.db` | projection store (droppable — rebuildable from substrates) |
| `studio_token` | `SCARCED_STUDIO_TOKEN` | unset | bearer token for quote issuance; unset disables those routes (fail-closed) |
| `sweep_seconds` | `SCARCED_SWEEP_SECONDS` | `30` | quote-expiry sweep cadence |
| `buzz.relay_url` | `SCARCED_BUZZ__RELAY_URL` | unset | community relay the M3 orchestrator connects to |

## Try it

The API is self-describing — start at the index:

```bash
curl -s localhost:7380/api/v1 | jq                 # every endpoint + schema links
curl -s localhost:7380/api/v1/schemas/rfq | jq     # JSON Schema of any wire type
```

Capture demand (open, no auth — this is the signal intake):

```bash
RFQ_ID=$(curl -s localhost:7380/api/v1/rfqs --json '{
  "query": "solana priority fee forecast api",
  "buyer_npub": "npub1vadgs8qfwsgf7ak3jqvsys6dprae6eyyzzwr8v345l39yz77af4s7eg4zn"
}' | jq -r .id)
```

Invalid input returns `422` with `{ "errors": [{ "field", "message" }] }`.

Issue the quote (studio-authenticated; one per RFQ — a second POST is `409`):

```bash
curl -s localhost:7380/api/v1/rfqs/$RFQ_ID/quote \
  -H 'authorization: Bearer dev-token' --json '{
  "price": { "amount": 250000000, "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" },
  "milestones": [
    { "title": "Forecast model", "description": "p50/p90 per program id",     "amount": 150000000 },
    { "title": "Gated endpoint", "description": "pay.sh-gated REST endpoint", "amount": 100000000 }
  ],
  "timeline": "2 weeks, weekly demos",
  "payout_destination": { "kind": "splits", "splits": [
    { "recipient": "CrewAgentA111111111111111111111111111111111", "bps": 10000 }
  ]},
  "channel": { "idle_timeout_seconds": 604800 },
  "expires_at": "2026-09-01T00:00:00Z"
}' | jq
```

The response carries the defaulted studio gate policy and its `policy_hash`
commitment. The buyer read is free: `GET /api/v1/rfqs/$RFQ_ID/quote` — status
is computed fail-closed against `expires_at`, so a lapsed quote reads
`LAPSED` even before the sweep stamps it.

Accept the quote (buyer, free, once — a second POST is `409`, and a lapsed
quote refuses):

```bash
curl -s -X POST localhost:7380/api/v1/rfqs/$RFQ_ID/quote/accept | jq .status
```

Acceptance stands in for funding while payments are stubbed (PLAN.md §6
override path): the contract starts.

## Watch it in Buzz

With the `buzz` config section present (see `scarced.example.yaml`), every
lifecycle beat is mirrored to the community relay: demand captured, quote
issued, and quote accepted post to the ops channel, and acceptance creates a
per-project workroom channel (`proj-<slug>-<shortid>`) where the
contract-starting post lands. The workroom's channel-create event id is
stored as the FUNDED → WORKROOM_ACTIVE evidence.

The daemon signs as the studio identity (`buzz.private_key`); a managed-agent
identity also needs the NIP-OA tag (`buzz.auth_tag`, env
`SCARCED_BUZZ__AUTH_TAG`). Omit the whole `buzz` section for a ledger-only
run.

## Develop

```bash
just ci        # fmt + clippy -D warnings + test — what CI runs
just schemas   # regenerate schemas/*.json from studio-types (drift-tested in CI)
just --list    # everything else
```
