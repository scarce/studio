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

## Run

```bash
just run                       # SCARCED_BIND (default 127.0.0.1:7380),
                               # SCARCED_DB (default sqlite://scarced.db)
curl http://127.0.0.1:7380/healthz
```

## Develop

```bash
just ci        # fmt + clippy -D warnings + test — what CI runs
just --list    # everything else
```
