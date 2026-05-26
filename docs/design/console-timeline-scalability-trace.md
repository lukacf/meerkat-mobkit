# Console Timeline Scalability Trace

## Requirements

| ID | Requirement | Phase | Runtime Caller | Evidence | Status |
| --- | --- | --- | --- | --- | --- |
| CONTRACT-001 | Timeline queries support explicit `since` and `recent` modes. | 0 | `mobkit/console/query_timeline`, `GET /console/timeline` | `cargo test -p meerkat-mobkit console_aggregator::store::tests`; `npm --prefix console run phase0:types --silent` | VALIDATED |
| CONTRACT-002 | Timeline responses expose continuation state beyond `frames` and `next_cursor`. | 0 | Console RPC/REST clients | `npm --prefix console run phase0:types --silent`; `npm --prefix console run e2e:browser --silent` | VALIDATED |
| REQ-001 | Global initial activity loads recent visible frames, not cursor 0. | 1 | `ConsoleApp` global activity seed | `browser global timeline recent seed ok` in `npm --prefix console run e2e:browser --silent` | VALIDATED |
| REQ-002 | Identity chat initial load shows recent usable history after one bounded request. | 1 | `ConsoleApp` agent chat panel | `browser chat pane recent first page ok` in `npm --prefix console run e2e:browser --silent` | VALIDATED |
| REQ-003 | Identity history rendering is incremental and does not wait for full transcript paging. | 2 | `ConsoleApp` identity timeline loader | `browser chat pane recent first page ok`; `browser chat pane older history demand paging ok` | VALIDATED |
| REQ-004 | Timeline paging skips invisible cursor gaps within a bounded raw scan budget. | 1 | `MobKitConsoleAggregator::query_timeline` | `cargo test -p meerkat-mobkit query_timeline_since_skips_hidden_raw_gaps_with_bounded_paging -- --nocapture` | VALIDATED |
| REQ-005 | Recent/tail queries use store-level reverse/indexed access, not whole-log replay. | 1 | `ConsoleLogStore::query_frames` | `cargo test -p meerkat-mobkit console_aggregator::store::tests -- --nocapture`; `fresh_timeline_snapshot_reads_tail_without_full_log_replay` | VALIDATED |
| REQ-006 | Identity recent snapshots keep a useful turn-start anchor before noisy recent tails. | 1 | `MobKitConsoleAggregator::query_timeline` | `cargo test -p meerkat-mobkit fresh_identity_snapshot_keeps_user_input_anchor_before_noisy_tail -- --nocapture` | VALIDATED |
| REQ-007 | Open-panel refresh uses continuation/recent semantics rather than repeating full oldest-first scans. | 2 | `ConsoleApp` refresh loop | `npm --prefix console run e2e:browser --silent`; source path uses initial `recent`, follow-up `since` by `latestTimelineCursor` | VALIDATED |
| E2E-001 | Browser E2E proves large global logs show recent worker activity. | 3 | Bundled console in Chromium | `browser global timeline recent seed ok` | VALIDATED |
| E2E-002 | Browser E2E proves delayed multi-page identity history renders before all pages finish. | 3 | Bundled console in Chromium | `browser chat pane recent first page ok`; `browser chat pane older history demand paging ok` | VALIDATED |
| E2E-003 | Browser/backend E2E proves invisible/empty cursor windows do not strand timeline loading. | 3 | Bundled console in Chromium plus aggregator entrypoint | `browser chat pane older history demand paging ok`; `query_timeline_since_skips_hidden_raw_gaps_with_bounded_paging` | VALIDATED |
| INV-001 | Timeline reads remain store-local and do not synchronously refresh broad session history. | 1 | Aggregator query path | `cargo test -p meerkat-mobkit query_timeline_is_store_local_for_registered_runtimes -- --nocapture`; `cargo test -p meerkat-mobkit` | VALIDATED |
| INV-002 | No Fugue-specific console path is introduced. | final | Repository scan | `rg -n "Fugue\|fugue\|LUC-631\|kapellmeister\|Kapellmeister" ...` only finds design-doc context/examples | VALIDATED |
| INV-003 | No Meerkat core/RPC changes are required for this bug. | final | Repository diff | `git diff --name-only` only shows MobKit console/backend files and design docs | VALIDATED |

## Typed But Unwired

- None.

## Phase Evidence

- `2026-05-26T20:59:26Z`, base `e4390bd`, branch `codex/console-timeline-scalability`.
- `cargo test -p meerkat-mobkit console_aggregator::store::tests -- --nocapture`: 7 passed.
- `cargo test -p meerkat-mobkit query_timeline -- --nocapture`: 5 passed before the hidden-gap test was added; full package run later covered the expanded set.
- `cargo test -p meerkat-mobkit timeline_snapshot -- --nocapture`: 3 passed.
- `cargo test -p meerkat-mobkit query_timeline_since_skips_hidden_raw_gaps_with_bounded_paging -- --nocapture`: 1 passed.
- `cargo test -p meerkat-mobkit`: passed, including 227 lib tests, integration tests, and doctests.
- `cargo clippy -p meerkat-mobkit --all-targets -- -D warnings`: passed.
- `npm --prefix console run phase0:types --silent`: 121 JS/type-normalization tests passed.
- `npm --prefix console run e2e:browser --silent`: passed, including `browser global timeline recent seed ok`, `browser chat pane recent first page ok`, and `browser chat pane older history demand paging ok`.
- `git diff --name-only`: changes are limited to MobKit console/backend files and design docs; no Meerkat core/RPC files changed.
- `rg -n "Fugue|fugue|LUC-631|kapellmeister|Kapellmeister" ...`: matches only the design docs' motivating context and examples, not runtime code.
