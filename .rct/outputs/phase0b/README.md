# Phase 0b Remediation Evidence

This folder records Product Owner direct execution evidence for the 0b remediation run.

## Mandatory build/lint

- `cargo check`
  - transcript: `cargo-check.txt`
- `cargo clippy --all-targets --all-features -- -D warnings`
  - transcript: `cargo-clippy.txt`

## Exact env-gated execution commands (authoritative)

- BigQuery gate check must fail fast when unset:
  - `env -u MOBKIT_RUN_P0B_BQ cargo test -p meerkat-mobkit-core --test phase0b_external -- --ignored p0b_t2_bigquery_real_boundary_on_king_dnn_training_dev`
  - log: `test-p0b-t2-env-unset.txt`
- BigQuery real external run (gate enabled):
  - `MOBKIT_RUN_P0B_BQ=1 cargo test -p meerkat-mobkit-core --test phase0b_external -- --ignored --nocapture p0b_t2_bigquery_real_boundary_on_king_dnn_training_dev`
  - log: `test-p0b-t2-env-set.txt`
  - stdout markers expected in transcript: `PROJECT_SELECTED`, `DATASET_CREATED`, `TABLE_CREATED`, `CLEANUP_BEGIN`, `CLEANUP_END`
- OIDC gate check must fail fast when unset:
  - `env -u MOBKIT_RUN_P0B_OIDC cargo test -p meerkat-mobkit-core --test phase0b_external -- --ignored p0b_t5_auth_oidc_jwks_endpoint_preflight`
  - log: `test-p0b-t5-env-unset.txt`
- OIDC real external run (gate enabled):
  - `MOBKIT_RUN_P0B_OIDC=1 cargo test -p meerkat-mobkit-core --test phase0b_external -- --ignored p0b_t5_auth_oidc_jwks_endpoint_preflight`
  - log: `test-p0b-t5-env-set.txt`
- Full Phase 0b ignored suite with both external gates enabled:
  - `MOBKIT_RUN_P0B_BQ=1 MOBKIT_RUN_P0B_OIDC=1 cargo test -p meerkat-mobkit-core --test phase0b_external -- --ignored`
  - log: `test-phase0b-all-env-set.txt`

## Phase 0b ignored tests executed

- `p0b_t1_rpc_method_matrix_boundary_preflight`
  - log: `test-p0b-t1.txt`
  - scope: request/response handled by a dedicated child-process RPC gateway binary (`phase0b_rpc_gateway`), not in-process handler calls.
- `p0b_t2_bigquery_real_boundary_on_king_dnn_training_dev` (gate unset)
  - expected fast-fail contract: `MOBKIT_RUN_P0B_BQ must be set to 1`
  - log: `test-p0b-t2-env-unset.txt`
- `p0b_t2_bigquery_real_boundary_on_king_dnn_training_dev` (`MOBKIT_RUN_P0B_BQ=1`)
  - log: `test-p0b-t2-env-set.txt`
  - probe implementation uses UUID-suffixed disposable dataset naming plus trap-based cleanup.
  - explicit cleanup evidence markers emitted: `CLEANUP_BEGIN:...` and `CLEANUP_END:...`.
- `p0b_t3_json_store_filesystem_lock_preflight`
  - log: `test-p0b-t3.txt`
- `p0b_t4_sse_stream_and_reconnect_preflight`
  - log: `test-p0b-t4.txt`
- `p0b_t5_auth_oidc_jwks_endpoint_preflight` (gate unset)
  - expected fast-fail contract: `MOBKIT_RUN_P0B_OIDC must be set to 1`
  - log: `test-p0b-t5-env-unset.txt`
- `p0b_t5_auth_oidc_jwks_endpoint_preflight` (`MOBKIT_RUN_P0B_OIDC=1`)
  - log: `test-p0b-t5-env-set.txt`
  - scope proven: OIDC discovery metadata retrieval and JWKS endpoint key availability.
- `p0b_t6_module_family_process_preflight`
  - log: `test-p0b-t6.txt`
  - scope: per-family dedicated Python subprocess probes (distinct scripts per family) + discovery + route viability.
- `p0b_t7_sdk_console_toolchain_and_payload_preflight`
  - log: `test-p0b-t7.txt`
  - scope: locally-runnable Node/Python payload contract checks against runtime RPC plus in-process console route contract checks.
- Full 0b ignored suite with gated externals enabled (`MOBKIT_RUN_P0B_BQ=1 MOBKIT_RUN_P0B_OIDC=1`)
  - log: `test-phase0b-all-env-set.txt`

## Checklist status policy

Checklist updates are performed only after independent 3-gate approval.

## Latest gate cycle reviewer artifacts

- Gate summary for this cycle: `gate-review-round-2026-02-28.md`
- Reviewer artifact 1: `gate-review-round-2026-02-28-reviewer-1.md`
- Reviewer artifact 2: `gate-review-round-2026-02-28-reviewer-2.md`
- Reviewer artifact 3: `gate-review-round-2026-02-28-reviewer-3.md`
