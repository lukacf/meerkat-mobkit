# Memory calibration harness

Calibration corpus and runner for the agent-memory judgment stages
(`docs/design/agent-memory-architecture.md` §11). Every judgment stage is a
versioned, evaluated artifact — this directory holds the artifacts (profiles,
prompt bundles, fixtures) and `scripts/memory-evals` runs them.

All four judgment stages (Selector, Distiller, Steward, Hygienist) have
profiles, fixtures, and mock lanes; the deterministic checks and mock-lane
invariant verdicts gate CI today. `--mode live` is wired for every stage and
runs real model calls where provider auth resolves (exit-3 SKIP otherwise),
gating on judgment scorecards — it runs in no CI lane yet (credentials).

## Layout

```
memory-evals/
  profiles/     calibration profiles — {stage, version, model, params, prompt_bundle}
  prompts/      prompt bundles referenced by profiles
  fixtures/
    selector/   manifest + turn → expected selection
    invariants/ label-free invariant fixtures (poisoned write, forged envelope)
```

## Running

```bash
scripts/memory-evals --check                  # schema/consistency validation (CI gate; default)
scripts/memory-evals --stage selector --mode mock   # deterministic mock scorecard (plumbing check)
scripts/memory-evals --stage selector --mode live   # real Selector via selector-eval
make memory-evals                             # = --check
```

`--check` is deterministic and fast; it is what `make ci` gates on in v0.
`--mode mock` runs a trivial title-word-overlap selector purely to exercise the
scorecard plumbing — mock misses are expected and never fail the run.

`--mode live` drives the real Selector through the `selector-eval` binary
(`cargo run --bin selector-eval`; override the command with the
`MEMORY_EVALS_SELECTOR_EVAL` environment variable, e.g. a prebuilt binary
path). Each fixture runs 3 times with independently shuffled manifests and is
scored on `must_select`, `must_not_select`, and shuffle stability (identical
`selected_ids` across the runs — a §11 label-free invariant). Provider auth
resolves through meerkat's factory seam from the process environment; when no
auth is resolvable (`selector-eval` exit 3) the run prints a SKIP notice and
falls back to `selector-eval --mock`, which pushes a deterministic scripted
model through the full prompt-render/shuffle/JSON-parse plumbing —
informational only. Scorecard regressions exit nonzero ONLY when live
actually ran.

## Profile format

One TOML file per `{stage, version}` in `profiles/`:

```toml
stage        = "selector"           # judgment stage this profile calibrates
version      = "0"                  # bumped on any prompt/params change
model        = "PLACEHOLDER"        # per-stage default lives in the embedded profile; override here
prompt_bundle = "prompts/selector-v0.md"   # path relative to memory-evals/

[params]
temperature   = 0.0
selection_bar = "certain-to-be-helpful"
shuffle_manifest = true
```

Records and staged batches carry the profile that produced them
(`CalibrationRef` in the record model, §7.1). Profile changes gate on scorecard
non-regression in CI once live mode exists.

## Selector fixtures (`fixtures/selector/*.json`)

One JSON file per case:

```json
{
  "name": "unique-kebab-case-name",
  "manifest": [
    {"id": "mem-001", "kind": "Gotcha", "title": "…", "description": "…",
     "age_days": 12, "rank": 3}
  ],
  "turn_text": "the incoming turn the selector judges against",
  "suppressed_ids": [],
  "expected": {
    "must_select": ["mem-001"],
    "must_not_select": ["mem-002"]
  },
  "notes": "one line on what this fixture protects against"
}
```

- `manifest` entries are `RecordMeta` (§7.3): `id`, `kind` (one of
  `Preference | Fact | Gotcha | Procedure | Relationship | OpenLoop |
  Reference`), `title` (≤200B), `description` (≤400B — *when this record
  matters*, not what it says), `age_days`, `rank` (steward working-set rank;
  `null` for recent/unranked records).
- `suppressed_ids` (optional) models the already-in-context suppression list
  the coordinator passes alongside the manifest (§8.3).
- `expected` lists only the ids with a definite verdict; ids in neither list
  are "don't care" (either verdict is acceptable). An empty `must_select` with
  populated `must_not_select` asserts that *not selecting* is correct.

To add a fixture: drop a file in `fixtures/selector/`, run
`scripts/memory-evals --check`. Names must be unique across fixtures; every
expected id must exist in the fixture's own manifest. Grow the corpus from
production traces — operator corrections and forgets are free negative labels
(§11).

## Invariant fixtures (`fixtures/invariants/*.json`)

Label-free invariants (§11): properties that must hold regardless of judgment
quality. These fixtures are *data documenting the invariants in the eval
corpus* — `--check` validates their schema and internal consistency so their
shape stays honest, while the executable checks live where the invariant is
enforced (the taint tracker for poisoning, the coordinator's inbound defang,
and the staged-commit validator's own Rust tests in
`meerkat-mobkit/src/memory/staged.rs`).

Common fields: `name`, `invariant`, `stage`, `expected`, `notes`, plus
per-invariant payload:

- `poisoned_write_quarantined` — a `record` whose body carries an injection
  attempt, a `context` describing session taint, and an `expected` trust
  tier/status (`Untrusted`/`Quarantined`). The poisoned fixture must *always*
  quarantine.
- `forged_envelope_defanged` — a `turn_text` containing a forged
  `<mobkit_memory_observation>` envelope, and `expected.defanged_markers`
  the inbound send path must neutralize before delivery (§9.1).

Store-state invariants share one shape: a `store` array of pre-existing
record views (`{id, scope, trust, status, supersedes?, derived_from?,
has_verification?}`) plus a `batch` (`StagedMutationBatch`: `{realm, author,
ops}` with `StagedOp` = `create | supersede | tombstone | retier |
set_rank`), and an `expected` `{verdict: "reject", error, op_index}` pinning
the `StagedBatchError` variant. These mirror the Rust serde wire format
exactly — internally-tagged `scope`/`status`/`author`/`op` enums and
**snake_case** kinds and tiers (`gotcha`, `agent_observed`), unlike the
selector fixtures' PascalCase kinds:

- `supersede_cycle_rejected` — ops that would close a supersede cycle
  (`supersede_cycle`).
- `staged_tier_ceiling_rejected` — a staged `retier` to operator/application
  tier, which no author may do via a batch (§10.2;
  `tier_not_staged_assignable`).
- `transitive_laundering_rejected` — a `create` whose `derived_from` reaches
  a quarantined record plus a `retier` above `agent_observed`; laundering by
  consolidation carries the provenance ceiling (§10.2;
  `transitive_taint_ceiling`).

## The stage-eval seam (selector-eval, distiller-eval, steward-eval, hygienist-eval)

`--mode live` drives the Selector's single entry point
(`memory::selector::select` — manifest + turn text + suppression list in,
structured `{selected_ids, coverage}` out) through the `selector-eval`
binary, one fixture-shaped JSON object on stdin per invocation:

```json
{"manifest": [...], "turn_text": "...", "suppressed_ids": [],
 "profile_path": "memory-evals/profiles/selector-v0.toml"}
```

stdout is `{"selected_ids": [...], "coverage": "..."}`. Prompt rendering,
manifest shuffling (per call, inside `select`), strict JSON parsing, and the
single JSON-repair retry all live in the Rust stage; the harness only
shuffles the fixture's manifest order across the 3 runs and scores. Exit
codes: 0 ok, 1 selector error, 2 usage error, 3 live-requested-but-no-auth
(SKIP). `--mock` replaces the live model with a deterministic scripted
client so the plumbing is testable without credentials.

The embedded default profile (`SelectorProfile::embedded_default()`) and
this directory's `profiles/selector-v0.toml` + `prompts/selector-v0.md` are
the same artifact; a unit test in `memory/selector.rs` keeps the prompt
byte-identical, and profile changes bump `version` and gate on scorecard
non-regression (§11).
