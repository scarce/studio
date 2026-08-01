# scarce-studio — Build Guidelines (draft-00)

**Status:** discussion draft
**Author:** archy, per ludovic (2026-08-01, in-channel)
**Companions:** `DESIGN.md` (economics), `ARCHITECTURE.md` (system shape),
`PLAN.md` (milestones). This document answers *"how does the studio build —
anything?"* It governs every project the studio ships, and the studio itself
is expected to conform to it.

---

## 1. Language and conventions

**Backend components are always Rust**, following the Solana Foundation
conventions encoded in the shared skills repo
[`solana-foundation/ai-skills`](https://github.com/solana-foundation/ai-skills):

| Skill | Governs | When |
|---|---|---|
| `sf-rust-skill` | workspace layout, error handling (`thiserror`), `tracing`, config, async patterns, testing, CI | **every backend crate — mandatory** |
| `sf-solana-clients-skill` | `@solana/kit` / Codama client code | client-side Solana work |
| `sf-solana-programs-skill` | Anchor/Pinocchio, IDL, program testing | on-chain work (rare — PLAN.md §6) |
| `sf-typescript-skill` | TS patterns, docblocks, monorepo | frontends and TS SDKs only |
| `sf-tech-docs-writer-skill` | README, API docs, llms.txt | every deliverable's docs |

Two consequences, so this is not advisory prose:

1. **Workroom agents load the applicable skills at staffing time.** A skill is
   part of the crew's operating context, not a wiki page — see §4 for how
   `skills.toml` and the agent registry wire this.
2. **The mechanical subset is gate-enforced.** `machine_check` gates
   (PLAN.md §2.1) hold every milestone to: `just ci` green at the demoed
   commit (fmt, `clippy -D warnings`, full-workspace tests), schema↔code
   conformance suites where a wire contract exists, and fail-closed health
   endpoints. These graduated from practice to requirement — M0/M1 of the
   studio's own build demonstrated each one.

House rules the studio's own milestones established, now required of every
project: **strict schemas** (unknown fields rejected loudly, 422 with
per-field errors), **restart-survival tests** for anything persistent,
**publish-then-commit ordering** for any row that cites substrate evidence,
and **explicit crypto-provider installation** in every binary entry point
(never inherit one from cargo feature unification).

## 2. AI-first project shape

Every deliverable is built AI-first. Concretely: **the product is a pure core
crate; every externally visible surface is an adapter over it.**

```
                 ┌───────────────────────────────┐
                 │           core crate           │  business logic, zero I/O,
                 │  (types, state, invariants)    │  exhaustively unit-tested
                 └───────┬───────────┬───────────┘
                         │           │
            ┌────────────▼──┐   ┌────▼──────────┐   ┌──────────────────┐
            │  ACP surface  │   │  MCP surface  │   │  HTTP surface     │
            │  (buzz-acp:   │   │  (tools for   │   │  gated with MPP   │
            │  agent-native,│   │  any MCP      │   │  via solana-pay-  │
            │  channel-     │   │  client)      │   │  kit — the paid,  │
            │  driven)      │   │               │   │  pay.sh-listed    │
            └───────────────┘   └───────────────┘   │  door             │
                                                    └──────────────────┘
```

- **Core crate.** The only place business logic lives. No I/O, no runtime,
  no protocol types — the same discipline as `studio-core`'s gate engine
  (pure `eval`, tested exhaustively before any I/O existed). If a behavior
  cannot be unit-tested without a network, it is in the wrong crate.
- **ACP surface.** Agent-native: the artifact can be added to a Buzz channel
  and driven conversationally (`buzz-acp` is the harness contract). This is
  what makes a deliverable *staffable* — usable by the same kind of agent
  that commissioned it.
- **MCP surface.** Tool-native: any MCP client (Claude, goose, …) can call
  the capability without knowing our stack.
- **HTTP surface, gated with MPP** using the
  [`solana-pay-kit`](https://github.com/solana-foundation/pay-kit) crate
  (`rust/crates/kit`): the `axum` feature provides the unified 402 gate that
  dispatches both MPP and x402; `server` provides verification; buyers use
  `client`. This surface is what pay.sh lists — the commissioned endpoint of
  DESIGN.md is always this door.

Rationale: **buyers are agents.** A deliverable a human can click but an
agent cannot call is not delivered. The three surfaces are the three ways an
agent consumes a capability today (as a teammate, as a tool, as a paid API);
the core/adapters split keeps them honest — one implementation, three doors,
no logic in any door.

The studio itself is the reference implementation of this shape
(`studio-core` + port crates + `scarced`); deviations in a deliverable need
the same justification a GatePolicy weakening would need — that is, they
don't happen.

## 3. Agent registry — `agents/`

The studio's roster is declared in-repo, one file per agent:
`agents/<name>.persona.md`.

**Format: Buzz's persona format, verbatim** (V7 spec, parsed by the
`buzz-persona` crate in the Buzz workspace) — we do not invent frontmatter.
YAML frontmatter carries every setting Buzz gives an agent; the markdown body
is the system prompt:

```markdown
---
name: ruben
display_name: ruben
description: Implementer — Rust backend, Buzz integration
model: anthropic:claude-fable-5        # provider:model-id
runtime: claude                        # ACP runtime id
skills:
  - sf-rust
temperature: 0.2
max_context_tokens: 200000
thread_replies: true
subscribe: ["#scarce-studio"]
---

You are ruben, the studio's implementer. …
```

Loader semantics (`scarced` startup):

1. `scarced` reads `agents/*.persona.md` via `buzz-persona` at boot and holds
   the parsed registry in memory — this is the roster the orchestrator staffs
   workrooms from (ARCHITECTURE.md §2.2).
2. **Parse is strict and boot is fail-closed.** `buzz-persona` rejects
   unknown frontmatter keys (`deny_unknown_fields`) — a typo is a startup
   error, not a silently dropped setting. A registry that does not parse is a
   `scarced` that does not start.
3. Because frontmatter rejects unknown keys, **studio-specific economics stay
   in `roster.toml`**, keyed by persona `name`: npub (+ Solana pubkey and
   key-binding attestation ref, ARCHITECTURE.md §5), skill tags for crew
   selection, day rate. Identity/settings are the runtime's concern
   (persona file); money is the studio's (roster). One join key: `name`.
4. The registry **configures** agents; it does not mint them. Agent creation
   on Buzz remains owner-reviewed (ARCHITECTURE.md §2.2) — provisioning a new
   npub is a human ceremony, after which its persona file is committed here.

## 4. Skills configuration — `skills.toml`

Skills are customizable per-studio (and strengthenable per-project) through a
single TOML file at the repo root:

```toml
# skills.toml — which conventions govern the builds
[skills.sf-rust]
source = "github:solana-foundation/ai-skills"
path   = "sf-rust-skill"
rev    = "<pinned commit>"          # always pinned — never a branch

[skills.sf-tech-docs]
source = "github:solana-foundation/ai-skills"
path   = "sf-tech-docs-writer-skill"
rev    = "<pinned commit>"

[defaults]
backend = ["sf-rust"]               # applied to every backend crew member
docs    = ["sf-tech-docs"]

[overrides]
# per-skill studio config, passed to the skill's context when loaded
# sf-rust = { edition = "2021" }
```

Rules:

1. **Pinned revs only.** A skill is a dependency of the build's correctness;
   an unpinned skill is an unpinned compiler.
2. **Personas reference skills by slug** (frontmatter `skills:`); the slug
   resolves through `skills.toml`. The persona says *which*; the TOML says
   *from where, at what version, with what config*.
3. **Projects may strengthen, never weaken.** A Quote may add skills for its
   engagement; removing a default skill is not a per-project option — the
   same strengthen-only rule the GatePolicy carries (PLAN.md §2.1.4).
4. *Recommended (not yet required):* hash `skills.toml` into the FUNDED
   commitment alongside the GatePolicy hash, so "which conventions governed
   this build" is tamper-evident the same way the engagement's procedural law
   is.

## 5. Enforcement

Guidelines that are not gates decay. The mapping:

| Guideline | Enforced by |
|---|---|
| Rust + sf-rust conventions | `machine_check`: `just ci` green at the demoed commit |
| schema↔code conformance | `machine_check`: conformance suite in CI |
| MPP-gated HTTP surface live | `machine_check`: endpoint answers its 402 challenge correctly (already specified, PLAN.md §2.1) |
| ACP/MCP surfaces present | milestone acceptance criteria in the Quote |
| registry/skills well-formed | `scarced` fail-closed boot (§3.2) |

---

*Grounding: persona format & strict parse —
`~/Coding/buzz/crates/buzz-persona/src/persona.rs` (PersonaConfig; frontmatter
`deny_unknown_fields`); `solana-pay-kit` features (`axum` unified 402 gate,
`server`, `client`) — `~/Coding/pay-kit/rust/crates/kit/Cargo.toml` (v0.4.0);
skills repo — `github.com/solana-foundation/ai-skills`; gate semantics —
`PLAN.md` §2.1.*
