# scarce-studio — Design Document (draft-00)

**Status:** discussion draft
**Author:** archy, with ludovic
**Date:** 2026-08-01

---

## 1. Thesis

pay.sh has a cold-start problem: agents search the catalog for a paid capability;
when the search misses, the interaction dies. Supply doesn't arrive because demand
is invisible; demand doesn't materialize because supply is thin.

**scarce-studio converts unmet demand into supply.** When a catalog search misses,
the fallback is not "sorry" — it is *"let's build it."* The buyer describes the
product, how it should be monetized, and the competitive landscape; pays upfront
into escrow; and the studio spins up a Buzz channel staffed with qualified agents
that build the capability and ship it as a gated pay.sh endpoint.

The studio is itself an instance of the loop it serves:

```
create → gate → discover → authorize → pay → consume → create again
```

scarce-studio manufactures the *create* step for others, and its own intake is a
gated pay.sh endpoint. The studio dogfoods the protocol end to end.

## 2. The cold-start problem, precisely

A two-sided market fails to bootstrap when each side's participation is contingent
on the other's prior participation. The classical escapes are subsidy (pay one side
to show up) or vertical integration (become one side yourself). scarce-studio is
the second escape, with a twist: it doesn't stockpile speculative supply — it
manufactures supply *against expressed, funded demand*. Every unit of supply the
studio creates is, by construction, something at least one buyer already paid for.

Corollary: **the failed search is the most valuable event in the system.** A catalog
miss is a structured, timestamped, identity-attributed statement of willingness to
pay for something that does not exist. v0 must capture every miss as a durable
demand record (query, intent, budget signal, buyer pubkey) even when the buyer does
not commission — aggregated misses justify speculative builds later, and they are
the studio's order book.

## 3. Engagement lifecycle

| Phase | What happens | Payment primitive |
|-------|-------------|-------------------|
| 0. Demand capture | pay.sh catalog miss → structured RFQ: what, monetization model, competition, budget ceiling | none (record only) |
| 1. Intake & quote | Studio agents assess feasibility; return quote: price, milestone schedule, timeline, proposed revenue splits for the artifact | `upto` (bounded feasibility fee, settle actual ≤ max) — or free, see Open Decision 3 |
| 2. Commitment | Buyer opens an MPP **session** channel; deposit = quoted price; splits committed at `open` | `session` open (escrow) |
| 3. Build | Buzz channel created; agents staffed; buyer present. Each milestone: artifact demoed in-channel → buyer signs cumulative voucher → next milestone begins | off-chain vouchers |
| 4. Delivery | Final artifact = gated pay.sh endpoint + repo + docs. Final voucher → `settleAndSeal` + `distribute` | cooperative close |
| 5. Operation | Endpoint earns on pay.sh; optional residual splits to studio/crew | the artifact's own channels |

## 4. Payment design

### 4.1 Why `session`

A multi-day, multi-milestone engagement is exactly the multi-settlement shape.
Two further points close the argument:

1. **Escrow.** An `upto` authorization is a signed permit; funds are pulled at
   settle time and can be gone by then. A session channel's deposit is locked
   on-chain at `open`. For an engagement measured in days, the studio must not
   carry buyer-solvency risk — escrow removes it.
2. **Exit symmetry.** Session gives both sides a unilateral, bounded exit
   (`requestClose` + grace period for the buyer; stop-work at the settled
   watermark for the studio). `upto` has no analogous structure.

`upto` remains the right shape for Phase 1 (a bounded feasibility study: "up to
$X, charged by actual effort, settled once") and for small fixed-scope builds
where the whole engagement fits one authorization window.

### 4.2 The voucher is the acceptance artifact

This is the design's core trust insight. Freelance marketplaces (Upwork et al.)
build elaborate custodial milestone-escrow and dispute machinery. The MPP session
channel *is* that machinery, minus the custodian:

- **Client-signed voucher mode** (the spec default): the buyer controls
  `authorizedSigner` and signs each cumulative voucher. Signing the voucher for
  milestone *k* **is** accepting milestone *k*. No oracle, no arbiter — the
  settlement trigger and the acceptance act are the same signature.
- **Studio exposure** is bounded by one milestone: work performed since the last
  settled voucher. If the buyer ghosts or refuses to sign, the studio stops; the
  idle timeout (max 30 days) drives cooperative close at the last watermark.
- **Buyer exposure** is bounded by what they've already accepted: `requestClose`
  starts the grace period, the studio settles any signed-but-unsubmitted
  vouchers, and `deposit − settled` refunds automatically.

Milestone granularity is therefore the *dispute-resolution parameter*. There is no
separate dispute system; there is only the question "how much work are both sides
willing to have at risk between signatures?" This should be negotiated at quote
time and priced accordingly (finer milestones = more buyer check-ins = higher
coordination cost = higher price).

Note the grace period is per-channel state set at `open` (u32 seconds). The spec's
RECOMMENDED 900s suits streaming APIs; a dev-shop engagement should set it to
**days** (e.g., 172800 = 48h) so a buyer's forced close cannot outrun a studio that
is asleep across time zones.

### 4.3 `distributionSplits` is trust-minimized payroll

The session spec lets `open` commit an ordered list of `{recipient, shareBps}`
(≤ 32 recipients, hash-committed on-chain). Bind the staffed crew there:

- Each contributing agent is a split recipient at their negotiated bps; the studio
  takes the implicit payee remainder.
- Every `distribute` pays each agent their cumulative floor delta **directly from
  escrow**. The studio never custodies crew wages. An agent joining a scarce
  engagement can verify its compensation on-chain before writing a line of code.

**Constraint to design around:** splits are immutable after `open` (hash-committed).
Staffing changes mid-engagement can't rebind them. Options:

- (a) **Channel-per-milestone**: fresh channel, fresh splits, per milestone. Clean
  but adds per-milestone open/close overhead and re-escrow friction.
- (b) **PDA payee**: the spec permits a PDA payee (cooperative close via CPI). Route
  the studio share to a studio program that re-splits programmatically under
  mutable, off-channel-visible rules. More machinery, full flexibility.
- (c) **Over-provision to a studio treasury** and pay adjustments out-of-band.
  Simplest, weakest trust story.

Recommendation for v0: (a) for engagements expected to re-staff, single channel
otherwise; grow into (b) when the studio program exists.

## 5. Trust model — failure matrix

| Failure | Mechanism | Bounded loss |
|---------|-----------|--------------|
| Studio stalls / underdelivers | Buyer `requestClose`; grace lets studio settle earned vouchers; auto-refund of remainder | Buyer loses ≤ 0 unaccepted work |
| Buyer ghosts (won't sign) | Studio stops at watermark; idle timeout → cooperative close | Studio loses ≤ 1 milestone of labor |
| Quality dispute | None needed: acceptance = signature; exposure = 1 milestone either way | Milestone granularity |
| Buyer insolvency | Impossible post-`open`: deposit escrowed on-chain | — |
| Studio absconds with crew wages | Impossible with splits: escrow pays crew directly | — |
| Sybil / unqualified agents | Reputation (see §7): settled-channel history + shipped gated endpoints, both publicly attributable | Staffing quality |

## 6. Identity: the two-keypair problem

Nostr identities are **secp256k1 Schnorr** keys; Solana identities are **Ed25519**.
An agent's Buzz reputation and its payment identity are *different keypairs* with
no intrinsic link. The studio needs a binding attestation — e.g., a Nostr event in
which the npub asserts a Solana pubkey, countersigned by an Ed25519 signature over
the npub (a bidirectional proof-of-control, in the spirit of NIP-39 external
identity claims). Without it, "pay the agent who did the work" is not actually
verifiable, and split recipients cannot be audited against channel membership.
This attestation format is small, load-bearing, and should be specified early —
it is also reusable by every future studio.

## 7. Staffing and reputation

"Qualified agents" must cash out to something inspectable. Buzz gives us the raw
material for free:

- **Transcripts are public work history.** Every engagement channel is an
  attributable record of who proposed, who built, who reviewed, who shipped.
- **Settled channels are receipts.** An agent's history of splits received is an
  on-chain, sybil-resistant record of *paid* work.
- **Shipped gated endpoints are a portfolio.** The catalog entry's revenue is a
  quality signal no résumé can fake.

v0 staffing can be curated (the studio owner picks the crew). The protocol-shaped
version — agents advertising skills, bidding on RFQs, staking on outcomes — is a
later layer and should not block launch.

## 8. Studio-agnostic evolution

For pay.sh to become studio-agnostic, the seam between *unmet demand* and *studio*
must be a protocol, not a product. scarce-studio should be built as the reference
implementation of four schemas it could publish as a companion draft:

1. **RFQ** — the structured demand record from a catalog miss (query, product
   description, monetization model, competition, budget ceiling, buyer pubkey).
2. **Quote** — price, milestone schedule, timeline, proposed splits, channel
   parameters (grace, idle timeout, mint).
3. **Escrow convention** — "engagement = MPP session channel with client-signed
   vouchers and crew splits," exactly §4 of this document.
4. **Delivery attestation** — signed claim binding the final artifact (endpoint
   FQN, repo, docs) to the RFQ and the settled channel.

If those four objects are spec'd, "let's build it with scarce" generalizes to
"let's build it with *any studio that speaks RFQ*" — and pay.sh's fallback becomes
a routing decision, not a partnership. scarce's durable advantage is then its
demand data and reputation, which is the correct kind of moat for a protocol
company to hold: earned, not extracted.

### 8.1 Tracked, no action: RFQ negotiation between micro-agents

*(per ludovic, 2026-08-01 — record the direction; build nothing yet)*

Today an RFQ meets exactly one quote, take-it-or-leave-it. The tracked end
state is that a new RFQ opens a **negotiation surface**: offers and
counteroffers exchanged agent-to-agent until one is accepted — because the
economy this serves is one of **deployed micro-agents** (the term going
forward: nobody deploys "apps"; they deploy micro-agents, each a small,
enumerated set of gated endpoints — exactly the deliverable shape
GUIDELINES.md §2 already mandates). Both sides of a negotiation are
micro-agents: the buyer's agent that missed the catalog, and the studio (or
studios) bidding to fill the miss.

What this decomposes into, when it is picked up:

1. **A bid is just a quote that competes.** The Quote schema (§8.2) already
   carries price, milestones, `expires_at`, and a gate policy; negotiation
   generalizes it from *the* quote to *a* bid among several, plus a
   `supersedes` reference for counteroffers. Accept then cites the winning
   bid's event id — the accept endpoint's shape survives unchanged.
2. **Negotiation history is substrate, like everything else.** Offers and
   counters are signed events attached to the RFQ; the projection invariant
   (ARCHITECTURE.md §1) extends to them for free, and the negotiation
   transcript becomes replayable evidence — which matters the day a dispute
   asks "what was actually offered?"
3. **Multi-party bidding is the deferred multi-studio routing** (Non-goals;
   PLAN.md §0) arriving through the front door: several studios speaking RFQ
   bid on one demand record. The seam is already protocol-shaped; negotiation
   is what makes the routing decision *priced* rather than configured.
4. **Layering:** a2a-style protocols are candidates for the conversational
   negotiation layer; MPP/x402 remain the settlement layer underneath.
   Nothing about negotiation touches escrow semantics — a session channel
   still opens only when one bid is accepted (§4).

The only thing worth doing early is keeping the seam cheap: quotes are
already versioned, expiring objects; nothing in the current schemas
forecloses "many quotes per RFQ, each referencing what it counters."

### 8.2 Tracked, no action: deployment economics — allowance, hosting, build loans

*(per ludovic, 2026-08-01 — record the direction; build nothing yet)*

Once deliverables are deployed micro-agents (Cloud Run behind the
payment-gated agent-gateway), the studio carries operational costs on the
buyer's behalf: container registry storage, image builds, the gateway proxy's
own compute, KMS signing, egress. Three ideas to hold together:

1. **The paywall burden is priced, not feared.** Running the gateway for
   every hosted artifact makes the studio a platform operator — SLA,
   metering audit, price-policy custody. The *dollar* cost is noise
   (gateway compute is itself scale-to-zero and per-request; registry
   storage is ~$0.10/GiB-month, so a distroless Rust image costs well under
   1¢/month — dead artifacts are nearly free to keep listed forever). The
   *liability* is real, and it is exactly what the operator split in the
   gateway spec's `splits` block is for: the platform fee is the price of
   being the paywall.
2. **Deployment allowance in the RFQ/Quote.** Registry + build + gateway
   onboarding + first-N-months hosting priced as an explicit line of the
   Quote ("an allowance to get things running"), not silently absorbed.
   Seam: the Quote schema grows an operations/allowance field when this is
   picked up; nothing forecloses it today.
3. **The build loan: financing as a split schedule.** If the buyer will not
   pay upfront, the studio may finance the build; the artifact's gate then
   routes **100% of revenue to the studio until the RFQ price is
   reimbursed**, after which the split flips to the engagement's steady
   state. The unifying observation: commission (buyer pays, buyer owns),
   co-op (residual splits), and loan (repayment waterfall, then flip) are
   all points on one line — **who finances the build determines the split
   schedule over time**. `payoutDestination` generalizes from a constant to
   a *schedule*; the flip is a threshold event on cumulative settled
   revenue, evidence-cited and hash-committed like the GatePolicy, so
   neither side can move the goalposts mid-repayment.

What makes the loan underwritable is the studio's own order book: aggregated
catalog misses are the demand signal that justifies fronting a build — the
speculative-builds question (§9.5) and the loan are the same credit decision
wearing different clothes. Risks recorded for the eventual design: demand
risk transfers to the studio (price it), buyer moral hazard when nothing is
at stake upfront (the intake fee stays), and the repayment cap must be
explicit (principal, principal×multiple, or time-boxed) before the first
loan is written.

## 9. Open decisions (need ludovic's call)

1. **Commission vs co-op ownership.** Does the buyer own 100% of the shipped
   endpoint's revenue (pure commission), or does the artifact's own gate carry
   residual splits to studio/crew (co-op)? `distributionSplits` makes the co-op
   model native and self-enforcing; it also changes the studio's economics from
   services revenue to portfolio revenue. This is the biggest fork in the model.
2. **Milestone granularity defaults.** Quote-time negotiable, but what's the
   default? Weekly? Per-deliverable? The parameter is the entire dispute system.
3. **Priced intake or free funnel?** An `upto`-gated feasibility fee filters spam
   and dogfoods the protocol, but adds friction exactly where the funnel is
   widest. (A possible middle path: free RFQ capture, priced quote.)
4. **The npub ↔ Solana-pubkey attestation** (§6): specify now as a mini-draft, or
   punt to curated-staffing v0 where the studio owner vouches manually?
5. **Speculative builds.** When aggregated misses show N buyers wanting the same
   capability, may the studio build on spec and gate it, with the misses' authors
   as launch customers? (This is where the demand records compound.)

## 10. v0 scope proposal

Deliberately small; every piece exercises the real protocol:

- Catalog-miss hook → RFQ record (flat files or a single table; no infra).
- One intake endpoint, gated with `upto`, returning a quote.
- One engagement, run by hand: session channel, curated crew, crew splits,
  buyer-signed milestone vouchers, cooperative close.
- One shipped artifact: a gated pay.sh endpoint that did not exist before.

The first completed loop — miss → RFQ → quote → escrow → build → gate → the
buyer's agent *consuming the endpoint it commissioned* — is the demo, the test
suite, and the pitch, in one artifact.

## Non-goals (v0)

- Automated staffing/bidding markets.
- On-chain reputation or staking.
- Dispute arbitration beyond the voucher mechanism.
- Multi-studio routing (design the seams; don't build the router).

---

*Grounding: MPP session — `~/Coding/mpp-specs/specs/methods/solana/draft-solana-session-00.md`
(escrow, vouchers, grace period, `distributionSplits`, idle timeout, PDA payee);
x402 upto — `~/Coding/x402/specs/schemes/upto/scheme_upto.md` (single-settlement,
time-bound, recipient binding).*
