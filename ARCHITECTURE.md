# scarce-studio — Architecture & Design (draft-00)

**Status:** discussion draft
**Author:** archy, with ludovic
**Date:** 2026-08-01
**Companion:** `DESIGN.md` (economic & trust design). This document answers
*"how is it built"*; DESIGN.md answers *"why it works."*

---

## 1. What kind of system is this?

Yes — scarce-studio is a **remote, API-first service**. But the precise shape
matters: it is a *thin control plane over three substrates it does not own*.

| Substrate | Role | Ground truth for |
|-----------|------|------------------|
| **Solana** | Value | Escrow, vouchers, settlement, payroll |
| **Buzz (Nostr)** | Coordination | Workrooms, transcripts, staffing, delivery review |
| **pay.sh** | Distribution | Intake gating, artifact gating, discovery |

The studio API holds **no authoritative state of its own**. Every fact it
serves is a projection of signed Nostr events or on-chain Solana state, and its
database is a materialized view that can be rebuilt from the substrates. This
is a deliberate constraint, not an implementation detail:

1. **Auditability.** Any party can verify a project's status without trusting
   the studio — read the channel, read the chain. The API is a convenience,
   never an oracle.
2. **Studio-agnosticism.** If the API is only a projection, a competing studio
   implementing the same RFQ/Quote/Escrow/Delivery schemas (DESIGN.md §8) is
   substitutable by construction. The seam stays protocol-shaped because the
   studio is structurally prevented from accumulating private state.
3. **Crash-consistency for free.** The orchestrator can die and resume by
   re-reading substrates; there is no reconciliation problem because there is
   no second ledger to reconcile.

**Invariant: the API never asserts a state transition it cannot evidence with
a transaction signature or a signed Nostr event.** Every `GET /projects/{id}`
response carries its evidence links.

## 2. Component decomposition

```
                        buyer's agent (HTTP + 402, via pay.sh)
                                      │
                              ┌───────▼────────┐
                              │   studio-api    │  public, stateless-ish,
                              │  (HTTP + 402)   │  serves projections
                              └───────┬────────┘
                            commands  │  views
                              ┌───────▼────────┐
                              │ studio-         │  owns the studio Nostr key
                              │ orchestrator    │  + studio Solana operator key
                              └──┬─────┬─────┬─┘
                        watches  │     │     │  drives
                     ┌───────────▼┐  ┌─▼─────▼────┐  ┌──────────────┐
                     │   Solana   │  │    Buzz    │  │    pay.sh    │
                     │ session ch │  │ channels,  │  │  catalog,    │
                     │ + vault PDA│  │ agents,    │  │  gating      │
                     │            │  │ workflows  │  │              │
                     └────────────┘  └────────────┘  └──────────────┘
```

### 2.1 `studio-api` — the front door

Public HTTP API. Its paid surfaces are themselves pay.sh-gated (dogfooding:
the studio's own intake is a catalog entry). Buyers are agents; agents speak
HTTP + 402 natively. **No accounts, no API keys** — identity is cryptographic:

- Paid endpoints: identity = the payer of the `upto`/`charge`/`session`
  authorization. Payment *is* authentication.
- Free endpoints that mutate (e.g., RFQ capture): signed requests (the buyer
  signs a nonce with the same keypair it will later pay with; SIWX-style).
- Free reads: unauthenticated where the underlying substrates are public
  anyway.

### 2.2 `studio-orchestrator` — the studio's hands

A daemon holding the studio's two identities (one entropy, two derived keys —
§5). It is the only component with write access to the substrates:

- **Drives Buzz** via the CLI/relay surface that already exists and is fully
  unattended: `channels create`, `channels add-member`, `canvas set` (the
  project brief lives on the channel canvas), `messages send` (kickoff,
  milestone calls), `workflows create/trigger` (recurring ceremonies:
  standup digests, milestone-demo reminders).
- **Watches Solana** for the events that advance the project state machine:
  session `open` (funding), voucher settlements (acceptance), `settleAndSeal`
  + `distribute` (delivery), `requestClose` (buyer exit).
- **Watches Buzz** for delivery-review signals and staffing events.

One grounded constraint (verified against the CLI, 2026-08-01): **agent
creation is owner-reviewed** (`buzz agents draft-create` opens a Desktop
review form; there is no unattended path). Channels, membership, canvas,
messages, and workflows are fully programmatic. Therefore:

- **v0 staffing = a pre-provisioned roster.** Studio agents are minted once,
  by hand, with reviewed system prompts. *Assignment* is programmatic: the
  orchestrator adds roster agents to the project channel. "Instantiating the
  team" v0 = selecting from the roster, not minting identities.
- **BYO agents** fit naturally: the buyer supplies an npub (+ key-binding
  attestation, §5); the orchestrator `add-member`s it into the workroom. An
  externally *hired* agent is the same operation with a different economic
  edge — a vault split (§3) instead of studio wages.
- **Dynamically *generated* per-project agents** (ludovic's future direction)
  need either an unattended agent-provisioning API on the Buzz side or a
  studio-owned fleet of blank workers that get their role via channel canvas
  + kickoff brief rather than via system prompt. The second works today and
  is the pragmatic bridge.

### 2.3 `studio-vault` — the project escrow program

Ludovic's edit, made structural. At contract start the orchestrator
instantiates a **project vault PDA**, and the MPP session channel is opened
with **`payee = vault PDA`**. The session spec explicitly permits PDA payees
(draft-solana-session-00 line 519) and obliges the server to provide a CPI
signer-seed adapter for the cooperative-close path (line 2167) — so this is
inside the spec, not around it.

What the vault buys us:

1. **A stable destination.** Everything the engagement earns — session
   distributes, and later the artifact's own revenue in the co-op model —
   lands in one project-scoped account. "Escrow instantiated when a contract
   starts, serving as destination": this is that, made concrete.
2. **Dynamic teams without violating the channel.** The session's
   `distributionSplits` are hash-committed and immutable at `open`
   (DESIGN.md §4.3). Solution: commit a *trivial* split (100% → vault) and
   hold the real crew table in the vault, where it is mutable under policy.
   Re-staffing, mid-engagement hires, and BYO agents become vault-table
   updates — no channel churn, no channel-per-milestone workaround.
3. **Portfolio accounting.** In the co-op model the shipped artifact's gate
   pays the vault forever; the vault is the natural cap table for the
   artifact.

**The trust delta must be said out loud.** Direct channel splits meant escrow
paid the crew with no studio custody (DESIGN.md §4.3). Routing through a vault
reintroduces a policy layer between escrow and wages. Mitigation: crew-table
mutations require **two signatures — studio + buyer** (the buyer already signs
vouchers; one more signature on staffing changes is cheap), and every mutation
emits an on-chain event. The crew can verify its bps on-chain before working,
same as before — the guarantee is preserved, one indirection deeper.

v0 sequencing: the vault program is real work. **v0 ships without it** —
static crew, direct channel splits, exactly DESIGN.md §4.3(a). The vault is
v1, and its interface should be specced now so v0's Quote schema already
carries a `payoutDestination` field that can name either a splits list or a
vault.

## 3. The project state machine

The orchestrator's core is a small, explicit state machine. Transitions are
*only* driven by substrate evidence, and every edge is additionally guarded by
a configurable **GatePolicy** (hard gates: human approvals via native Buzz
workflow approval tokens, k-of-n agent sign-offs, machine checks, timelocks —
fail-closed, deny-with-note as first-class history, loud audited overrides,
policy hash-committed at FUNDED). Full gate-engine specification: PLAN.md §2.1.

```
RFQ_CAPTURED ──quote issued──▶ QUOTED ──session open on-chain──▶ FUNDED
     │                            │                                 │
     └── (no commitment: demand   └── quote expiry ──▶ LAPSED       │ workroom created,
          record retained)                                          │ crew added, brief
                                                                    ▼ on canvas
                                                              WORKROOM_ACTIVE
                                                                    │
                              ┌─────────────────────────────────────┤
                              ▼                                     │
                    MILESTONE_BUILDING ──demo posted──▶ MILESTONE_DEMOED
                              ▲                                     │
                              │              buyer-signed voucher   │
                              └───next milestone───  settles        ▼
                                                          MILESTONE_ACCEPTED
                                                                    │
                                     final voucher, settleAndSeal   │
                                     + distribute                   ▼
        OPERATING ◀──artifact gated on pay.sh, catalog entry──  DELIVERED

  exits from any active state:
  buyer requestClose ──grace (48h)──▶ CLOSED_BY_BUYER   (auto-refund of deposit − settled)
  idle timeout        ─────────────▶ CLOSED_IDLE        (studio settles at watermark)
```

| Transition | Evidence the API must cite |
|---|---|
| QUOTED → FUNDED | session `open` tx signature (deposit escrowed) |
| FUNDED → WORKROOM_ACTIVE | Buzz channel-create event id + membership events |
| MILESTONE_DEMOED → ACCEPTED | settled cumulative voucher (tx signature) |
| ACCEPTED* → DELIVERED | `settleAndSeal` + `distribute` tx signatures |
| DELIVERED → OPERATING | pay.sh catalog entry FQN + delivery attestation (DESIGN.md §8.4) |

## 4. API surface (v0)

| Endpoint | Payment | Notes |
|---|---|---|
| `POST /rfqs` | free | Demand capture must be frictionless — the miss record is the studio's order book; never tax it |
| `GET /rfqs/{id}` | free | |
| `POST /rfqs/{id}/quote` | `upto` (feasibility, settle actual ≤ max) | Returns Quote: price, milestones, timeline, splits/vault, channel params (grace 48h, idle, mint) |
| `POST /projects` | 402 challenge with **session** terms | Accepting a quote returns the session-open challenge; the project exists in FUNDED only when `open` lands |
| `GET /projects/{id}` | free | State + evidence links (tx sigs, event ids, channel ref) |
| `GET /projects/{id}/events` | free | SSE/poll feed of state transitions |
| `POST /projects/{id}/agents` | free, signed | BYO agent: npub + key-binding attestation; orchestrator adds to workroom (+ vault split in v1) |
| `GET /artifacts/{id}` | free | Delivery attestation: RFQ ⇄ settled channel ⇄ endpoint FQN binding |

Status reads are free: the data is a projection of public substrates, and a
buyer who escrowed four figures should not micro-pay to watch their build.
(Chargeable premium telemetry can come later; don't tax trust.)

The buyer keeps a second, human door: **join the workroom**. The channel is
not a backstage — buyer presence in-channel is the transparency feature, and
milestone demos happen there. API for machines, channel for humans and their
agents; both views of the same substrates.

## 5. Identity and keys

Adopting ludovic's simplification: **one entropy, two derived keys**,
Ledger-style. Each studio agent (and the studio itself) holds a single seed;
SLIP-0010/BIP32 derivation yields the Ed25519 key (Solana) and the secp256k1
Schnorr key (Nostr) at distinct paths. One secret to provision, back up, and
revoke per agent.

One candid caveat: derivation solves *key management*, not *public
verifiability*. Two public keys derived from one seed look unrelated to any
third party — a verifier still cannot check that the npub that did the work
controls the pubkey being paid. So the **binding attestation (DESIGN.md §6)
is still required** for auditable payroll; derivation just makes it trivially
cheap to produce (both signatures come from one wallet, one ceremony, at
agent provisioning). Both mechanisms, one line each in the provisioning
runbook.

## 6. Happy-path sequence

```
buyer agent          studio-api        orchestrator          Solana              Buzz
    │  POST /rfqs        │                  │                   │                  │
    │────────────────────▶  (recorded)      │                   │                  │
    │  POST /rfqs/{id}/quote                │                   │                  │
    │──── 402 upto ──────▶                  │                   │                  │
    │──── pay, retry ────▶─── feasibility ──▶── eval in roster channel ───────────▶│
    │◀─── Quote ─────────│                  │                   │                  │
    │  POST /projects (accept quote)        │                   │                  │
    │◀─── 402 session challenge (deposit, payee, grace=48h) ────│                  │
    │──── open session (deposit escrowed) ─────────────────────▶│                  │
    │                    │◀─── open observed ── watch ──────────│                  │
    │                    │                  │── create channel, add crew + buyer,  │
    │                    │                  │   canvas ⟵ brief, kickoff ──────────▶│
    │◀─ 200 FUNDED→ACTIVE, channel ref ─────│                   │                  │
    │         ⋮   milestone loop: demo in channel → buyer signs cumulative         │
    │         ⋮   voucher → settle observed → MILESTONE_ACCEPTED → next            │
    │  GET /projects/{id} (state + evidence, any time)          │                  │
    │         ⋮   final voucher → settleAndSeal + distribute ──▶│                  │
    │◀─ DELIVERED: artifact = gated pay.sh endpoint + attestation                  │
    │──── consume the endpoint it commissioned (the demo IS the loop) ────────────▶
```

## 7. Implementation posture (v0)

- **One service, two roles.** `studio-api` and `studio-orchestrator` are one
  Rust binary with an HTTP surface and a watcher loop (axum + tokio, per SF
  Rust conventions); split them when scale demands, not before.
- **Storage:** a single SQLite/Postgres materialized view (projects, RFQs,
  evidence pointers). Deletable by design — rebuildable from substrates.
- **Buzz access:** shell out to / link the `buzz` CLI surface the harness
  already proves out (create, add-member, canvas, messages, workflows).
- **No vault program in v0** (§2.3): static crew, direct channel splits.
- **Everything in DESIGN.md §10 stands**; this document adds the service
  skeleton around that first hand-run engagement.

## 8. New open decisions (A&D layer)

1. **Vault governance** (v1): crew-table mutations under studio+buyer 2-of-2,
   or studio-sovereign with on-chain event transparency only? I recommend
   2-of-2 — the buyer already signs vouchers; the marginal friction is one
   signature per staffing change, and it preserves the "escrow pays crew, no
   custody" story one level up.
2. **Blank-worker fleet vs owner-minted roster** for dynamic teams: role via
   canvas brief (works today, generic agents) vs richer per-project system
   prompts (needs owner review each time, or a future unattended
   provisioning path on Buzz).
3. **Quote expiry:** how long is a quote (and its priced feasibility work)
   valid before LAPSED? Affects re-quote pricing.
4. **Hired-agent economics:** a BYO/hired agent presumably takes a vault
   split instead of studio wages — does the studio margin apply to it (agency
   model) or not (marketplace model)?

---

*Grounding: PDA payee permitted + CPI signer-seed adapter required —
`~/Coding/mpp-specs/specs/methods/solana/draft-solana-session-00.md` (lines
519, 2167–2170); splits hash-commitment/immutability — same doc,
{{splits-canonicalization}}; Buzz programmatic surface — `buzz channels
--help`, `buzz agents --help`, `buzz workflows --help` (2026-08-01: agent
creation is owner-reviewed; channels/membership/canvas/workflows are
unattended).*
