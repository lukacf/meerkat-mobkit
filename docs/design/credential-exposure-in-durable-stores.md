# Credential Exposure in Durable Stores — Substitution at the Mint, Detection at Ingress

**Status:** proposal · **Target:** meerkat (tool-execution boundary) + mobkit 0.8.19 (console ingress)
**Explicitly NOT 0.8.18.** 0.8.18 is a paired repin on meerkat 0.8.24; this spans two repos, a
tool-argument contract and three durable stores, and must be decided on its own terms.
**Basis:** measured, 2026-08-17, across two production fleets and mobkit source. Every claim
below is either a code citation or a fleet measurement with its sample size attached.

---

## 1. What was found

Mobkit persists **unredacted tool-call arguments** into the console frame store, on every
deployment, and has done so for as long as that store has existed. Two independent paths, both
mobkit's:

- `console_aggregator/mod.rs:4319` — the session-history projection writes `"args": args`
  verbatim into the `tool_call_requested` frame payload.
- `console_aggregator/mod.rs:3907` `frame_from_console_event` — the live path copies the
  event's entire `data` object into the frame payload wholesale, so
  `ToolCallRequested { id, name, args }` lands complete. No field selection, no filter.

There is no redaction seam anywhere upstream to rely on: `meerkat-core/src/event.rs:174`
defines `ToolCallArguments` as `#[serde(transparent)] struct ToolCallArguments(Value)` — raw
JSON carried verbatim — and `redact`/`scrub`/`secret` appear **zero** times in meerkat's event
vocabulary.

### Field measurement

**HomeCore** (household fleet, 17 agents), 8,165 `tool_call_requested` frames in a live console
store — a SQLite file on a box. Classified without printing values:

| class | count | note |
|---|---|---|
| sql-ish patterns | 84 | 81 of them `shell` |
| bearer/authorization | 22 | mostly agents *talking about* authorization — peer messages are prose |
| absolute filesystem paths | 22 | all `shell` |
| URLs with query strings | 16 | |
| **token-shaped values** | **0** | no `ghp_`, `sk-`, `ya29.` |

**OB3** (production, 30 days), 89,040 `tool_call_requested` frames: 304 credential-shaped
matches, **all 304 the plain words** ("secret" 261, "password" 35, "bearer" 7, "API-key" 1) in
initiative and POR text. Zero `xox*`/`Bearer`/`-----BEGIN`/`ya29.`/`AIza` values.

**Verdict: structurally exposed, no credential material found on either fleet.**

Two caveats that both fleets stated themselves and which this document keeps:

1. HomeCore validated the detector with a **known-positive control** before believing the zero
   — planted `api_key`, `client_secret`, a `ghp_` and an `sk-` token, confirmed all four fired,
   confirmed ordinary prose ("authorization for the school trip") stayed clean. The zero is
   worth something *because* the instrument was shown able to produce a non-zero.
2. Both are pattern-based absences. A bare hex or base64 credential with no recognisable prefix
   matches nothing either fleet searched for. And both fleets' tools happen to be benign — a
   camera entity id, a calendar id, a BigQuery read. As HomeCore put it: *we are the fleet that
   got lucky, not the fleet that was careful.* The next adopter's tool is a database client or
   an HTTP fetcher with a bearer token in an argument.

---

## 2. Framing correction — the store is confidential by nature

The first framing of this defect was "the console store is a lower-protection location than the
transcript." **That is wrong and should not survive into the design.** A household SQLite store
legitimately contains sensitive family data that must be there and must be agent-readable.
Confidentiality is not the distinguishing property.

The actual defect is narrower: mobkit makes **an additional durable copy, governed by no
policy**, of data that another store in the same codebase already refuses to hold.

---

## 3. Where "forever" actually lives

Three distinct exposures, which need separating because a mitigation aimed at one does nothing
for the others.

| layer | exposure | bounded? | encryption-at-rest helps? |
|---|---|---|---|
| 1. Disk at rest | stolen box, backups, a file copied off, query output pasted into a shared channel | **unbounded** | yes |
| 2. Provider transmission | transcript is the message list sent to the model | **bounded** — only while in the active window; compaction drops it. Provider-side retention ~30 days | no |
| 3. Agent readback | anything with the key sees plaintext, and agents must; an agent can quote a value into a peer message or a log line | **unbounded** | no |

An earlier draft of this analysis treated layer 2 as the dominant one. It is not: context is not
forever and old transcripts are not resent. **The unbounded exposure is layers 1 and 3 — the
durable stores and agent readback.**

Encryption at rest is therefore genuine defence-in-depth for layer 1 (and worth noting: all
three fleets spent 2026-08-17 querying these files directly and pasting results onto a shared
bus). It is *not* a substitute for the fix, it does nothing for layers 2 or 3, and it carries
real costs: a SQLCipher build change, key management and rotation, and the loss of exactly the
direct-query debugging that produced every finding in this document.

---

## 4. Precedent already in the codebase

Mobkit has **already decided this question** for one store. `meerkat-mobkit/src/memory/secrets.rs`
implements curated gitleaks-class scanning on the memory write path — four pattern classes
(AWS access key ids, GitHub tokens, private-key headers, credential assignments), plain
deterministic scanning, no regex dependency, no entropy heuristics. Its stated rationale:

> an agent that saw a live credential in tool output must not be able to persist it into durable
> memory, where it would re-enter every future build's context and every read surface.

Its posture is **refuse the write and name the class, never silently redact**, enforced at the
staged-validator chokepoint and the proposal seam.

That reasoning applies verbatim to the transcript and to the console store. The policy exists on
the *smallest* blast-radius store and is absent from the two larger ones. Whatever is decided
here should be consistent with it, or should consciously supersede it.

---

## 5. Proposal A (meerkat) — credential substitution at the tool boundary

**The mechanism.** A tool argument carries a *handle*, not a value. The harness resolves it
locally at execution time:

```
curl 'http://camera.local/snap?pwd=$pwd353353'
                                   └─ resolved at execution via getPassword(mycamera)
```

The transcript, and therefore every downstream copy, only ever contains `$pwd353353`.

**Why this shape and not a per-store policy.** It fixes the value at the point it is minted, so
every consumer inherits the fix for free — transcript, console frames, memory, provider
transmission, HomeCore's backfill, and any store nobody has built yet. A per-store redaction
policy solves the problem N times and leaves the N+1th writer to rediscover it. This is the same
argument meerkat applied to the duplicated tool-result body: *an inline cap at a downstream
writer does not fix this; it still writes the body twice, only smaller, and every consumer
reimplements the cap.*

**Ownership.** This is meerkat's: it lives at the tool-execution boundary and mobkit cannot
perform the substitution.

**Design questions this raises, none of them blocking:**

- *Discovery.* The model must know a handle exists in order to emit it. That implies a listing
  surface ("which secrets can I reference?"), which is itself a capability decision.
- *No adversarial containment.* `getPassword(mycamera)` resolves to a real value in the agent's
  hands; nothing prevents it echoing that into its own text. This reduces **incidental**
  leakage — which is nearly all real leakage — and is not a boundary against the model.
- *Scope of substitution.* Whether the handle is resolved for every tool or only for tools
  declared to take credentials.

---

## 6. Proposal B (mobkit 0.8.19) — detection at console ingress

Substitution does not reach a credential a human pastes into chat for convenience. That path
never crosses the tool boundary.

**Mobkit already ships the detector.** `detect_secret` is `pub`, operates on free text, and has
a validated four-class pattern set. Pointing it at console ingress catches the paste at the
point of entry.

Its existing posture is also the right one here: **refuse and name the class, never silently
redact.** Silently mangling a human's message is worse than telling them what was detected —
and a refusal the caller can act on ("that looks like a GitHub token") is precisely the shape
the refusal doctrine requires.

**Ownership.** Entirely mobkit's; needs nothing from upstream and does not block on Proposal A.

---

## 7. What is *not* proposed

- **Redacting tool arguments from the transcript.** Arguments in the transcript are how a future
  turn learns correct tool usage. Removing them degrades agent capability. The substitution
  design preserves this: `$pwd353353` in an argument still demonstrates the call's shape.
- **Redacting the console copy as the primary fix.** Verified: nothing reads console frames back
  into agent context (`SessionHistory` frames are projected *from* the transcript *into* the
  console, one direction). So a console-side redaction costs *human debugging* and buys only
  layer-1 reduction on one of three copies. Worth doing if Proposal A is declined; not worth
  doing instead of it.
- **Encryption at rest as the answer.** See §3. Defence-in-depth, not a fix.

---

## 8. Recommended sequencing

1. **0.8.19, mobkit, independent:** Proposal B — `detect_secret` at console ingress.
2. **meerkat, own timeline:** Proposal A — substitution at the tool boundary. The larger and
   more valuable change.
3. **Only if A is declined:** console-side argument policy — tool *name* always, arguments only
   by host allow-list per tool. The platform cannot know which argument is a credential; the
   host can.
4. **Independently, whenever:** encryption at rest, as layer-1 defence-in-depth, on its own
   cost/benefit.

---

## Appendix — how the args projection nearly became a platform default

During the 2026-08-17 storage-tier design discussion, the mobkit lead proposed that tier-1
metadata carry "a projection of the tool call arguments", derived by joining on the tool id that
`ToolCallRequested` / `ToolResultReceived` / `ToolExecutionCompleted` all share — and then
argued it should be **mandated** rather than optional.

The motivation was sound: HomeCore's camera frames carry `source: "inline"` and nothing else, so
the source label they needed lives in the arguments. OB3 independently converged on the same
field as a *re-fetch key*, since their oversized results are re-reads of durable state.

It was retracted after the meerkat lead checked the type. Mandating it would have made
"copy unredacted tool input into a durable store" the platform default for every adopter.

The reusable lesson: **the proposal was generalised from the one tool in front of us.**
`get_camera_snapshot`'s arguments are an entity id — harmless. Nobody asked what the *worst*
tool's arguments look like until the type was read. The measurement then showed the hazard was
not something about to be introduced; it was already there, in 8,165 rows.
