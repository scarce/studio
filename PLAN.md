# scarce-studio — Implementation Plan (draft-00)

**Status:** ready for an implementer
**Author:** archy, with ludovic
**Date:** 2026-08-01
**Companions:** `DESIGN.md` (economics & trust), `ARCHITECTURE.md` (system shape)

---

## 0. Scope ruling (read first)

Per ludovic (2026-08-01): **custom on-chain work is out of scope for now.**
That ruling simplifies the build more than it might appear, because it exposes
the real critical path:

> **scarce-studio v0 is ~80% a Buzz orchestration service.** Payments are an
> *integration* of rails that already exist (pay.sh gating, MPP `upto`, MPP
> `session` used exactly as specified — static crew, direct
> `distributionSplits`), and they sit behind a feature flag so every earlier
> milestone is demonstrable without touching a chain.

**Building now:** the API, the demand ledger, the quote flow, the workroom
orchestrator, the project state machine, the stub-mode engagement loop, then
live payments as the last integration.

**Explicitly deferred** (designed-for, not built): vault PDA program and its
2-of-2 governance; dynamic agent minting; attestation *verification* (the
field is recorded, not enforced); reputation; multi-studio routing;
speculative builds. Each has a seam in the schemas below so none requires a
breaking change later.

## 1. Repo layout

Rust, per SF conventions (workspace, axum, tokio, sqlx/SQLite, thiserror,
tracing). One binary; split later only if scale demands. House rules for
*everything the studio builds* — languages, skills, AI-first project shape —
live in `GUIDELINES.md`; this section is only the studio's own tree.

```
scarce-studio/
├── Cargo.toml                 # workspace
├── crates/
│   ├── studio-core/           # domain types, schemas, state machine (no I/O)
│   ├── studio-store/          # SQLite projections (rebuildable by design)
│   ├── studio-buzz/           # BuzzPort trait + CLI-backed impl + mock
│   ├── studio-pay/            # PayPort trait: stub impl now, live impl in M5
│   └── studio-api/            # axum routes, auth, SSE
├── src/main.rs                # `scarced`: HTTP server + orchestrator loop
├── schemas/                   # rfq.json, quote.json, escrow-terms.json,
│                              # gate-policy.json, delivery-attestation.json
├── agents/                    # roster registry: <name>.persona.md, Buzz
│                              # persona format, loaded at boot (GUIDELINES §3)
├── skills.toml                # pinned skill sources + defaults (GUIDELINES §4)
├── roster.toml                # crew economics keyed by persona name:
│                              # npub, skill tags, day rate
└── justfile                   # build, test, lint, run, integration-test
```

Two port traits are the load-bearing abstraction:

- **`BuzzPort`** — `create_channel`, `add_member`, `set_canvas`,
  `send_message`, `create_workflow`, `poll_events`. **Preferred impl: depend
  on the Buzz workspace crates directly** (per ludovic, 2026-08-01; verified
  against `~/Coding/buzz/Cargo.toml`):
  - `buzz-sdk` — typed Nostr event builders for Buzz operations (channels,
    messages, mentions) — the write path;
  - `buzz-core` — core types, **event verification**, filter matching — use
    it to verify gate-evidence events (signatures, kinds) instead of trusting
    relay echoes;
  - `buzz-ws-client` — relay subscriptions for `poll_events` (push, not poll);
  - `buzz-workflow` — the YAML-as-code engine (`schema.rs`) — emit
    schema-valid workflow definitions incl. approval steps rather than
    hand-templating YAML;
  - `buzz-test-client` — the integration/E2E client, tailor-made for the
    env-gated M3 relay suite.
  The `buzz` CLI remains the reference for expected behavior and a fallback
  impl if a crate seam is not public enough. Fully mockable either way, so
  the state machine is testable without a relay.
- **`PayPort`** — `funding_status(project)`, `acceptance_status(milestone)`,
  `close_status(project)`. **M0–M4 ship the stub impl** (funding = operator
  override; acceptance = buyer's signed channel message). **M5 swaps in the
  live impl** (session `open` / voucher settlement / `settleAndSeal` observed
  via RPC). The state machine cannot tell the difference — that is the point.

## 2. State machine (single source of movement)

```
RFQ_CAPTURED → QUOTED → FUNDED → WORKROOM_ACTIVE
                  │                    │
                  └→ LAPSED            └→ { BUILDING → DEMOED → ACCEPTED }×N
                                                   │
                                       DELIVERED ←─┘ (final acceptance)
                                                   → OPERATING
   exits: CLOSED_BY_BUYER | CLOSED_IDLE  (from any active state)
```

Every transition records **evidence** — in stub mode a signed Nostr event id;
in live mode a tx signature. The API never asserts a state it cannot cite.

| Transition | Stub-mode evidence (M0–M4) | Live-mode evidence (M5) |
|---|---|---|
| QUOTED → FUNDED | operator override, reason logged | session `open` tx sig |
| FUNDED → WORKROOM_ACTIVE | channel-create + membership event ids | same |
| DEMOED → ACCEPTED | buyer's signed "accept milestone k" channel message | settled cumulative voucher tx sig |
| ACCEPTED → DELIVERED | studio delivery message + attestation JSON | `settleAndSeal` + `distribute` tx sigs |

### 2.1 Gate engine (per ludovic, 2026-08-01: workflows must be very strong, with configurable hard gates incl. human validation)

Evidence says a transition *happened*; gates say whether it is *allowed to
happen*. Every state-machine edge carries an ordered list of gates from the
project's **GatePolicy**; the orchestrator may advance only when **all** gates
on the edge pass. Gate evaluation is a pure function in `studio-core` —
`eval(edge, policy, evidence_set) → Pass | Blocked(missing…)` — the
orchestrator only collects evidence; it never decides.

Gate types (v0):

| Gate | Satisfied by | Evidence recorded |
|---|---|---|
| `human_approval` | Named principal approves. Primary mechanism: **native Buzz workflow approval** — the orchestrator's workflow step emits an approval request with a token UUID; the human resolves it (`buzz workflows approve --token … --approved true\|false --note …`). Fallback: signed channel message with fixed grammar | consumed token id / signed event id, note |
| `agent_signoff` | k-of-n signed sign-off messages from named crew (e.g., reviewer agent) | k event ids |
| `machine_check` | Verifiable predicate: CI green at commit X, endpoint answers its 402 challenge correctly, schema validation passes | CI run URL + commit hash, probe transcript |
| `payment_evidence` | `PayPort` status (stub: operator record; live: tx sig) | per §2 table |
| `timelock` | Minimum elapsed review window since the edge became eligible | timestamps |

Hard-gate semantics — the properties that make it "very, very strong":

1. **Fail-closed.** Missing, ambiguous, or expired evidence blocks the
   transition. A gate timeout **escalates** (notification to the gate's
   principal, then to the studio owner); it never auto-passes.
2. **Denial is a recorded outcome, not an absence.** A `human_approval` deny
   (with note) rolls the milestone back to BUILDING, attaches the note to the
   project record, and requires a fresh demo to re-enter DEMOED. Denials are
   first-class history, visible in `GET /projects/{id}`.
3. **No silent bypass.** The only escape hatch is an operator override that is
   itself a gate event: authenticated, reason-required, permanently visible in
   the evidence trail. Overrides are loud by design — an audit of a project
   that used one should be embarrassing, not impossible.
4. **The policy is committed at funding.** The GatePolicy (negotiated in the
   Quote, defaults from studio config) is hash-recorded at the FUNDED
   transition. Weakening it mid-engagement is impossible; strengthening it
   requires both parties' signed consent, recorded as evidence. This mirrors
   the splits hash-commitment pattern: DESIGN.md established that milestone
   granularity is the dispute system — **the GatePolicy is the engagement's
   procedural law, and it is tamper-evident.**

Default policy (studio config; buyers may strengthen per-project in the Quote):

```yaml
edges:
  QUOTED->FUNDED:        [payment_evidence]
  DEMOED->ACCEPTED:      [human_approval(buyer)]          # + agent_signoff(reviewer, 1-of-1) if quoted
  ACCEPTED->DELIVERED:   [machine_check(endpoint-live), human_approval(buyer), timelock(24h)]
  any->CLOSED_BY_BUYER:  [timelock(grace: 48h)]           # the session grace period, mirrored off-chain
```

## 3. Milestones

Each milestone ends in a runnable demo. Sizes assume one implementer.

### M0 — Skeleton *(≈1 day)*
- Workspace scaffold above; config from env; `GET /healthz`; SQLite migrations;
  CI: `fmt`, `clippy -D warnings`, `test`; justfile.
- **Done when:** `just ci` green; `curl :PORT/healthz` → 200.

### M1 — Demand ledger *(≈1–2 days)*
- `studio-core::Rfq` (query, product description, monetization model,
  competition[], budget ceiling, buyer npub, created_at) + `schemas/rfq.json`.
- `POST /rfqs` (validate, persist), `GET /rfqs/{id}`, `GET /rfqs?since=…`.
- No payment gating, no signature requirement yet — never tax the order book.
- **Done when:** curl round-trip; invalid RFQ → 422 with field errors; records
  survive restart; the pay.sh catalog-miss fallback can be pointed at
  `POST /rfqs` (even manually) and misses accumulate.

### M2 — Quote flow *(≈2 days)*
- `studio-core::Quote`: price, mint, milestones[{title, description, amount}],
  timeline, `payoutDestination` (v0: splits list; enum-shaped so `vault` slots
  in later), channel params (grace default **172800s**, idle timeout),
  **`gatePolicy`** (per §2.1; defaults from studio config, buyer may
  strengthen), `expires_at`.
- `studio-core` gate engine: `GatePolicy` types + pure `eval` function +
  `schemas/gate-policy.json`. Exhaustive unit tests here — every gate type,
  every blocked/deny/override path — before any I/O exists to confuse things.
- `POST /rfqs/{id}/quote` (studio-authenticated route; the quote itself is
  authored by a human/agent for now), buyer `GET /rfqs/{id}/quote`.
- Expiry sweep: QUOTED → LAPSED.
- **Done when:** RFQ→QUOTED→LAPSED observable via API with timestamps; quote
  validates against `schemas/quote.json`.

### M3 — Workroom orchestration *(≈3–5 days — the critical path)*
- `studio-buzz`: `BuzzPort` + CLI-backed impl + mock.
- `POST /projects` accepts a quote; in stub mode FUNDED is entered via an
  authenticated operator override (reason logged). On FUNDED the orchestrator:
  1. `channels create` (name from RFQ slug),
  2. `add-member` each roster agent named in the quote + the buyer's npub,
  3. `canvas set` ← the **brief** (rendered from RFQ + Quote: goal,
     monetization, competition, milestone schedule, acceptance protocol),
  4. kickoff message mentioning the crew,
  5. `workflows create` for the engagement workflow: milestone ceremonies
     **and the gate steps** — each `human_approval` gate materializes as a
     workflow approval step whose token the orchestrator tracks (native
     `buzz workflows approve` resolves it; verified against the CLI).
- Persist every returned event id as transition evidence; wire the gate
  engine in front of every transition — the orchestrator attempts an edge,
  gets `Blocked(missing…)`, and surfaces exactly what is missing in
  `GET /projects/{id}` (a blocked project must be self-explanatory).
- `GET /projects/{id}` (state + evidence links + channel ref),
  `GET /projects/{id}/events` (poll/SSE).
- `roster.toml` loading; crew selection is a field of the Quote (manual v0).
- **Done when:** one API call yields a live, staffed Buzz channel with the
  brief on canvas — demonstrated against the real relay (env-gated
  integration test) and fully covered against the mock; a gated transition
  visibly **blocks** until its approval token is resolved, and an operator
  override leaves a loud, permanent evidence row. **This milestone is the
  "how hackable is Buzz" answer, made executable.**

### M4 — Engagement loop, stub payments *(≈2–3 days)*
- Milestone lifecycle: studio posts demo in-channel → buyer posts a signed
  acceptance message (fixed grammar: `accept milestone <k> <project-id>`) →
  orchestrator's `poll_events` recognizes it → BUILDING→DEMOED→ACCEPTED with
  the event id as evidence.
- Delivery: artifact record {endpoint FQN, repo URL, docs URL} +
  `delivery-attestation.json` signed by the studio npub, binding RFQ ⇄
  engagement ⇄ artifact.
- Exits: buyer close request (channel message in stub mode) and idle timeout
  both drive terminal states.
- Exercise the gate engine's unhappy paths for real: one milestone **denied**
  with a note (rollback to BUILDING, fresh demo required), one gate timeout
  escalating to the studio owner, one loud operator override.
- **Done when:** a full engagement — RFQ → quote → fund(override) → workroom
  → 2 milestones (one denied first, then accepted) → delivery — runs
  end-to-end with only API calls, workflow approvals, and channel messages,
  and `GET /projects/{id}` tells the whole story with evidence links,
  including the denial. This is the *demo to show people*, and no chain was
  involved.

### M5 — Payments, live mode *(≈3–5 days; integration only, nothing invented)*
- `studio-pay` live impl:
  - Intake: gate `POST /rfqs/{id}/quote` with `upto` via pay.sh's existing
    gating (feasibility fee, settle actual ≤ max) — flag-controlled.
  - Funding: `POST /projects` returns the session-open 402 challenge (terms
    from the Quote: deposit, direct `distributionSplits` = static crew from
    roster, grace 48h); orchestrator watches RPC for `open` → FUNDED.
  - Acceptance: settled cumulative voucher observed → ACCEPTED.
  - Close: `settleAndSeal` + `distribute` observed → DELIVERED; escrow pays
    the crew directly per splits (the DESIGN.md §4.3 story, unchanged).
- Everything is the session/upto spec used as published — **no custom
  program, no vault** (that remains v-later; `payoutDestination` already
  carries the seam).
- **Done when:** one real engagement settles on devnet end-to-end; flag off
  reverts cleanly to stub mode.

### M6 — Close the loop *(≈1 day)*
- The commissioned artifact is a gated pay.sh endpoint; the buyer's agent
  consumes it. Record the consumption as the final line of the demo script.
- **Done when:** the sequence in ARCHITECTURE.md §6 has happened once, for
  real. That transcript is the pitch.

## 4. Testing strategy

- `studio-core` state machine: exhaustive unit tests, zero I/O — every
  transition, every rejection (e.g., acceptance for a non-DEMOED milestone).
- `studio-buzz`/`studio-pay`: contract tests against mocks; one env-gated
  integration suite per port (real relay; devnet) run in CI nightly, not on
  every push.
- Evidence discipline as a test: for any project in any state, every past
  transition row must carry a non-null evidence pointer. Property-test it.
- Rebuildability as a test (M3+): drop the SQLite file, replay from relay +
  (M5) RPC, assert identical projection. This keeps ARCHITECTURE.md §1 honest.

## 5. Order of work & dependencies

```
M0 → M1 → M2 → M3 → M4 → M6(stub demo possible here)
                      └──→ M5 → M6(live)
```

M5 is deliberately last and flag-isolated: if it slips, everything before it
still demos. An implementer should reach the M4 full-loop demo before opening
the payments integration at all.

## 6. What the implementer should NOT do

- No vault/escrow program, no Anchor project, no new on-chain code.
- No dynamic agent creation (owner-reviewed only today); roster is a TOML file.
- No attestation verification — record the field, enforce later.
- No reputation, bidding, or multi-studio routing.
- No premature service split — one binary until it hurts.

---

*Grounding: programmatic Buzz surface verified via `buzz channels|agents|workflows --help`
(2026-08-01, agent creation owner-reviewed); session/upto semantics —
`~/Coding/mpp-specs/specs/methods/solana/draft-solana-session-00.md`,
`~/Coding/x402/specs/schemes/upto/scheme_upto_svm.md`; system shape —
`ARCHITECTURE.md`; economics — `DESIGN.md`.*
