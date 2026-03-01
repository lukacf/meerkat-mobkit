# MobKit RCT-Lite Traceability (Phases 0-14)

## Requirement Traceability Matrix

| REQ-ID       | Phase | Implemented In                                           | Runtime Caller                        | Test Name                                                    | Status |
|--------------|-------|----------------------------------------------------------|---------------------------------------|--------------------------------------------------------------|--------|
| TYPE-001     | 0     | crates/meerkat-mobkit-core/src/types.rs                 | run_module_boundary_once + normalize_event_line | type_001_event_envelope_and_events_round_trip / external_valid_json_line_from_subprocess_parses | WIRED  |
| TYPE-002     | 0     | crates/meerkat-mobkit-core/src/types.rs                 | run_module_boundary_once (ModuleConfig boundary input) | type_002_module_config_and_restart_policy_round_trip / external_valid_json_line_from_subprocess_parses | WIRED  |
| TYPE-003     | 0     | crates/meerkat-mobkit-core/src/types.rs                 | run_discovered_module_once (MobKitConfig + DiscoverySpec + PreSpawnData) | type_003_bootstrap_and_discovery_round_trip / type_003_runtime_config_resolution_is_typed / external_valid_json_line_from_subprocess_parses | WIRED  |
| TYPE-004     | 0     | crates/meerkat-mobkit-core/src/rpc.rs, src/runtime.rs   | run_process_json_line -> run_rpc_capabilities_boundary_once -> parse_rpc_capabilities | contract_005_rpc_capabilities_requires_contract_version / external_rpc_capabilities_requires_contract_version_from_process_response | WIRED  |
| MK-001       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| MK-002       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| MK-003       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| MK-004       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| MK-005       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| MK-006       | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path | WIRED  |
| CONTRACT-001 | 0     | crates/meerkat-mobkit-core/src/process.rs, src/runtime.rs, src/protocol.rs | run_process_json_line -> run_module_boundary_once | contract_001_valid_event_line_parses / external_valid_json_line_from_subprocess_parses / external_unexpected_payload_from_subprocess_is_rejected | WIRED  |
| CONTRACT-002 | 0     | crates/meerkat-mobkit-core/src/runtime.rs, src/protocol.rs | normalize_event_line / run_module_boundary_once | contract_001_schema_invalid_response_rejected / contract_001_unexpected_type_payload_rejected / contract_002_malformed_event_lines_rejected_with_typed_errors / external_invalid_schema_from_subprocess_rejected / external_unexpected_payload_from_subprocess_is_rejected | WIRED  |
| CONTRACT-003 | 0     | crates/meerkat-mobkit-core/src/process.rs               | run_process_json_line -> run_module_boundary_once | external_timeout_on_never_responding_subprocess | WIRED  |
| CONTRACT-004 | 0     | crates/meerkat-mobkit-core/src/baseline.rs, src/runtime.rs | run_process_json_line -> run_meerkat_baseline_verification_once -> verify_meerkat_baseline_symbols | external_meerkat_baseline_symbols_check_against_repo_path / external_meerkat_baseline_missing_symbols_has_typed_diagnostics | WIRED  |
| CONTRACT-005 | 0     | crates/meerkat-mobkit-core/src/rpc.rs, src/runtime.rs   | run_process_json_line -> run_rpc_capabilities_boundary_once -> parse_rpc_capabilities | contract_005_rpc_capabilities_requires_contract_version / external_rpc_capabilities_requires_contract_version_from_process_response | WIRED  |
| CHOKE-001    | 0     | crates/meerkat-mobkit-core/src/runtime.rs               | normalize_event_line -> run_module_boundary_once | choke_001_mixed_agent_and_module_lines_normalize_through_shared_runtime_path / external_normalization_path_over_mixed_subprocess_outputs | WIRED  |
| REQ-001      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | start_mobkit_runtime + tracked child ownership in MobkitRuntimeHandle::shutdown | req_001_startup_ordering_and_graceful_shutdown_kills_tracked_children / req_001_config_error_when_discovery_references_unknown_module | WIRED  |
| REQ-002      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | start_mobkit_runtime_with_options -> supervise_module_start (policy budgets) | req_002_supervisor_transitions_and_restart_policy_enforced_with_budgets | WIRED  |
| REQ-003      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | normalize_event_line source-consistency enforcement + start_mobkit_runtime -> merge_unified_events | req_003_event_bus_merges_agent_and_module_events_with_deterministic_order / req_003_attribution_integrity_rejects_source_event_mismatch | WIRED  |
| REQ-004      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | route_module_call | req_004_and_req_005_router_parity_library_and_rpc_with_typed_unloaded_error | WIRED  |
| REQ-005      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | route_module_call + route_module_call_rpc_json + route_module_call_rpc_subprocess (shared core route path) | req_004_and_req_005_router_parity_library_and_rpc_with_typed_unloaded_error | WIRED  |
| REQ-006      | 4     | crates/meerkat-mobkit-core/src/rpc.rs                   | handle_mobkit_rpc_json (JSON parse vs request-shape classification) | rpc_001_invalid_requests_return_jsonrpc_errors | WIRED  |
| REQ-007      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | start_mobkit_runtime + MobkitRuntimeHandle::shutdown lifecycle + tracked child retirement | req_001_startup_ordering_and_graceful_shutdown_kills_tracked_children | WIRED  |
| REQ-008      | 1     | crates/meerkat-mobkit-core/src/runtime.rs               | start_mobkit_runtime_with_options -> supervise_module_start (MCP tools/list health semantics + transitions) | req_002_supervisor_transitions_and_restart_policy_enforced_with_budgets | WIRED  |
| STORE-001    | 5     | crates/meerkat-mobkit-core/src/runtime.rs               | BigQuerySessionStoreAdapter::stream_insert_rows + read_latest_rows/read_live_rows | phase5_bigquery_adapter_process_path_and_dedup_tombstone_semantics | WIRED |
| STORE-002    | 5     | crates/meerkat-mobkit-core/src/runtime.rs               | JsonFileSessionStore::append_rows (file I/O + lock handling + stale-lock recovery) | phase5_json_store_recovers_stale_lock_and_persists_rows / phase5_json_store_blocks_on_fresh_lock | WIRED |
| DEC-001      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | build_runtime_decision_state -> validate_bigquery_naming | dec_001_dec_003_dec_005_dec_006_dec_007_runtime_decision_state_wiring | WIRED  |
| DEC-003      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | build_runtime_decision_state -> load_trusted_mobkit_modules_from_toml | dec_001_dec_003_dec_005_dec_006_dec_007_runtime_decision_state_wiring | WIRED  |
| DEC-002      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | handle_console_rest_json_route -> enforce_console_route_access | dec_004_console_rest_route_uses_auth_middleware_policy / dec_002_auth_provider_mismatch_and_bypass_branches / dec_002_auth_provider_mismatch_branch | WIRED  |
| DEC-004      | 2     | crates/meerkat-mobkit-core/src/runtime.rs               | handle_console_rest_json_route (typed REST JSON + auth middleware) | dec_004_console_rest_route_uses_auth_middleware_policy / dec_004_console_route_bypass_when_app_auth_disabled | WIRED  |
| DEC-005      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | build_runtime_decision_state -> validate_runtime_ops_policy | dec_001_dec_003_dec_005_dec_006_dec_007_runtime_decision_state_wiring | WIRED  |
| DEC-006      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | build_runtime_decision_state -> validate_runtime_ops_policy (reject SLO enforcement) | dec_006_reject_slo_enforcement_in_v01 | WIRED  |
| DEC-007      | 2     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs, docs/rct/release-targets.json | build_runtime_decision_state -> parse_release_metadata_json + validate_release_metadata | dec_001_dec_003_dec_005_dec_006_dec_007_runtime_decision_state_wiring / dec_007_invalid_metadata_branches | WIRED  |
| AUTH-001     | 7     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | enforce_console_route_access / handle_console_rest_json_route | phase7_auth_001_provider_support_model_and_dec_002_default_behavior / choke_105_auth_to_console_route_target_defined_red | WIRED |
| AUTH-002     | 7     | crates/meerkat-mobkit-core/src/decisions.rs, src/runtime.rs | enforce_console_route_access / handle_console_rest_json_route | phase7_auth_002_allowlist_and_service_identity_path / e2e_701_auth_flow_target_defined_red | WIRED |
| AUTH-003     | 7     | crates/meerkat-mobkit-core/src/auth.rs, src/lib.rs | validate_jwt_locally (SDK-side local validation, no IPC) | phase7_auth_003_local_jwt_validation_without_ipc | WIRED |
| CONSOLE-001  | 8     | crates/meerkat-mobkit-core/src/runtime.rs | handle_console_rest_json_route (`/console/experience`) capability contract for base/module panels + activity feed | phase8_console_001_capability_driven_rendering_contract / e2e_801_console_experience_target_defined_red | WIRED |
| DEC-004      | 8     | crates/meerkat-mobkit-core/src/runtime.rs | handle_console_rest_json_route shared auth middleware for console surfaces (`/console/modules` + `/console/experience`) | phase8_console_002_auth_protected_access_remains_enforced / choke_105_auth_to_console_route_target_defined_red | WIRED |
| REQ-003      | 8     | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs | subscribe_events SSE/event envelope contract surfaced via `/console/experience` activity feed schema | phase8_req_003_choke_104_unified_activity_feed_contract_over_events / choke_104_event_bus_to_sse_bridge_target_defined_red | WIRED |
| MOD-001      | 9     | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs | evaluate_schedules_at_tick + MobkitRuntimeHandle::dispatch_schedule_tick + scheduling param validation (`validate_schedules`) + `mobkit/scheduling/*` RPC handlers | phase9_schedule_001_timezone_and_interval_evaluation_is_deterministic / phase9_schedule_003_rpc_rejects_invalid_interval_timezone_and_duplicate_schedule_id / choke_106_scheduling_dispatch_handoff_target_defined_red / e2e_901_scheduled_action_flow_target_defined_red | WIRED |
| REQ-002      | 9     | crates/meerkat-mobkit-core/src/runtime.rs | supervisor transition signal wiring into scheduling dispatch payload/events | phase9_req_002_choke_106_supervisor_restart_signal_is_wired_into_dispatch | WIRED |
| MOD-002      | 10    | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs | `MobkitRuntimeHandle::resolve_routing` + `mobkit/routing/resolve` | phase10_choke_107_routing_resolve_hands_off_to_delivery_send / choke_107_routing_to_delivery_handoff_target_defined_red | WIRED |
| MOD-003      | 10    | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs | `MobkitRuntimeHandle::send_delivery` + `delivery_history` + `mobkit/delivery/*` RPC handlers | phase10_e2e_1001_routing_delivery_flow_history_and_rate_limit / phase10_rpc_invalid_params_for_routing_and_delivery_are_typed / e2e_1001_routing_delivery_flow_target_defined_red | WIRED |
| SDK-001      | 11    | sdk/typescript/src/index.ts, sdk/typescript/scripts/parity.js, crates/meerkat-mobkit-core/tests/phase11.rs | `MobkitTypedClient` + `buildConsoleModulesRoute` + `defineModuleSpec` (TS) via subprocess parity harness | phase11_sdk_001_sdk_002_choke_110_and_e2e_1101_parity_contracts / e2e_1101_sdk_parity_flow_target_defined_red | WIRED |
| SDK-002      | 11    | sdk/python/meerkat_mobkit_sdk/client.py, sdk/python/meerkat_mobkit_sdk/helpers.py, sdk/python/scripts/parity.py, crates/meerkat-mobkit-core/tests/phase11.rs | `MobkitTypedClient` + `build_console_modules_route` + `define_module_spec` (Python) via installed-package parity harness | phase11_sdk_001_sdk_002_choke_110_and_e2e_1101_parity_contracts / e2e_1101_sdk_parity_flow_target_defined_red | WIRED |
| MOD-004      | 12    | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs | `evaluate_gating_action` + `decide_gating_action` + `gating_audit_entries` + `mobkit/gating/*` RPC handlers | phase12_r3_approval_flow_enforces_approver_constraints_and_audits / phase12_risk_tiers_and_timeout_fallback_are_wired_with_audit / choke_108_gating_to_approval_flow_target_defined_red / e2e_1201_gating_flow_target_defined_red | WIRED |
| MOD-005      | 13    | crates/meerkat-mobkit-core/src/runtime.rs, src/rpc.rs, tests/phase13.rs | `memory_stores` + `memory_index` + `memory_query` + Elephant endpoint health boundary + atomic rollback on backend failure | phase13_memory_rpc_index_query_and_store_counts_are_wired / phase13_elephant_memory_backend_persists_across_runtime_restart / phase13_elephant_memory_backend_endpoint_failure_maps_to_typed_rpc_error / choke_109_memory_to_gating_conflict_target_defined_red / e2e_1301_memory_gating_flow_target_defined_red | WIRED |

## Phase 0b External Contract Verification

| REQ-ID      | Phase | Test Name                                         | Status |
|-------------|-------|---------------------------------------------------|--------|
| RPC-001     | 0b    | p0b_t1_rpc_method_matrix_boundary_preflight       | WIRED  |
| RPC-002     | 0b    | p0b_t1_rpc_method_matrix_boundary_preflight       | WIRED  |
| STORE-001   | 0b    | p0b_t2_bigquery_real_boundary_on_king_dnn_training_dev | WIRED  |
| STORE-002   | 0b    | p0b_t3_json_store_filesystem_lock_preflight       | WIRED  |
| STORE-003   | 0b    | p0b_t3_json_store_filesystem_lock_preflight       | WIRED  |
| SSE-001     | 0b    | p0b_t4_sse_stream_and_reconnect_preflight         | WIRED  |
| SSE-002     | 0b    | p0b_t4_sse_stream_and_reconnect_preflight         | WIRED  |
| AUTH-001    | 0b    | p0b_t5_auth_oidc_jwks_endpoint_preflight          | WIRED  |
| AUTH-002    | 0b    | p0b_t5_auth_oidc_jwks_endpoint_preflight          | WIRED  |
| AUTH-003    | 0b    | p0b_t5_auth_oidc_jwks_endpoint_preflight          | WIRED  |
| MOD-001     | 0b    | p0b_t6_module_family_process_preflight            | WIRED  |
| MOD-002     | 0b    | p0b_t6_module_family_process_preflight            | WIRED  |
| MOD-003     | 0b    | p0b_t6_module_family_process_preflight            | WIRED  |
| MOD-004     | 0b    | p0b_t6_module_family_process_preflight            | WIRED  |
| MOD-005     | 0b    | p0b_t6_module_family_process_preflight            | WIRED  |
| SDK-001     | 0b    | p0b_t7_sdk_console_toolchain_and_payload_preflight| WIRED  |
| SDK-002     | 0b    | p0b_t7_sdk_console_toolchain_and_payload_preflight| WIRED  |
| CONSOLE-001 | 0b    | p0b_t7_sdk_console_toolchain_and_payload_preflight| WIRED  |

Gate governance status: `COMPLETED` with independent reviewer evidence package recorded for the current cycle.  
Latest review artifact: `.rct/outputs/phase14/gate-review-round-2026-03-01-rerun3.md`

## Phase 3c Red Target Definitions

| REQ-ID    | Phase | Test Name                                         | Status             |
|-----------|-------|---------------------------------------------------|--------------------|
| CHOKE-101 | 4     | choke_101_rpc_ingress_target_defined_red          | GREEN |
| CHOKE-102 | 4     | choke_102_module_router_handoff_target_defined_red| GREEN |
| CHOKE-103 | 5     | choke_103_session_store_handoff_target_defined_red| GREEN |
| CHOKE-104 | 6     | choke_104_event_bus_to_sse_bridge_target_defined_red | GREEN |
| CHOKE-105 | 7     | choke_105_auth_to_console_route_target_defined_red| GREEN |
| CHOKE-106 | 3c    | choke_106_scheduling_dispatch_handoff_target_defined_red | GREEN |
| CHOKE-107 | 3c    | choke_107_routing_to_delivery_handoff_target_defined_red | GREEN |
| CHOKE-108 | 3c    | choke_108_gating_to_approval_flow_target_defined_red | GREEN |
| CHOKE-109 | 3c    | choke_109_memory_to_gating_conflict_target_defined_red | GREEN |
| CHOKE-110 | 4     | choke_110_sdk_contract_mapping_target_defined_red | GREEN |
| E2E-401   | 4     | e2e_401_rpc_surface_target_defined_red            | GREEN |
| E2E-501   | 5     | e2e_501_session_persistence_target_defined_red    | GREEN |
| E2E-601   | 6     | e2e_601_sse_experience_target_defined_red         | GREEN |
| E2E-701   | 7     | e2e_701_auth_flow_target_defined_red              | GREEN |
| E2E-801   | 8     | e2e_801_console_experience_target_defined_red     | GREEN |
| E2E-901   | 3c    | e2e_901_scheduled_action_flow_target_defined_red  | GREEN |
| E2E-1001  | 3c    | e2e_1001_routing_delivery_flow_target_defined_red | GREEN |
| E2E-1101  | 3c    | e2e_1101_sdk_parity_flow_target_defined_red       | GREEN |
| E2E-1201  | 3c    | e2e_1201_gating_flow_target_defined_red           | GREEN |
| E2E-1301  | 3c    | e2e_1301_memory_gating_flow_target_defined_red    | GREEN |
| E2E-1401  | 3c    | e2e_1401_program_smoke_target_defined_red         | GREEN |

## Typed But Unwired (carry forward)

- None at final gate: program-wide typed-but-unwired carry-forward list is empty.

## Phase 3 Closure Checks

- Typed-but-unwired list: empty.
- Blocked statuses present: none (`MISSING/DEFERRED/STUBBED` absent).
- Full suite evidence:
  - `cargo check --workspace`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --workspace`
  - `cargo test --workspace -- --ignored`

## Phase 5 Remediation Evidence

- `crates/meerkat-mobkit-core/tests/phase5.rs` validates:
  - JSON file store concrete persistence, fresh-lock blocking, and stale-lock recovery.
  - BigQuery adapter concrete process path (`insert` + `query`) with dedup/tombstone semantics on read.
- Updated Phase 3c targets:
  - `choke_103_session_store_handoff_target_defined_red` now validates concrete JSON lock path and BigQuery table path wiring.
  - `e2e_501_session_persistence_target_defined_red` now exercises concrete JSON + fake-BigQuery backend APIs for latest-row and tombstone semantics.
- Logs:
  - `.rct/outputs/phase5/cargo-test-phase5.txt`
  - `.rct/outputs/phase5/cargo-test-phase3c-choke-103.txt`
  - `.rct/outputs/phase5/cargo-test-phase3c-e2e-501.txt`

## Phase 6 Remediation Evidence

- Updated Phase 3c targets:
  - `choke_104_event_bus_to_sse_bridge_target_defined_red` now validates concrete SSE transport output from `mobkit/events/subscribe`, including runtime-emitted keep-alive comment frame output and deterministic `id`/`event`/`data` event framing.
  - `e2e_601_sse_experience_target_defined_red` now validates bounded replay/backfill semantics, per-agent scoped stream filtering via `scope="agent"` + `agent_id`, checkpoint replay on the selected agent stream, and typed JSON-RPC invalid-params errors for missing/invalid `agent_id` plus out-of-window checkpoints.
- Logs:
  - `.rct/outputs/phase6/cargo-test-phase3c-choke-104.txt`
  - `.rct/outputs/phase6/cargo-test-phase3c-e2e-601.txt`

## Phase 7 Remediation Evidence

- Updated auth implementation:
  - `crates/meerkat-mobkit-core/src/decisions.rs` now supports configured provider identities for `GoogleOAuth`, `GitHubOAuth`, and `GenericOidc`, preserves DEC-002 default-provider enforcement, and adds a concrete service-identity authorization path (`AuthProvider::ServiceIdentity` + `svc:` principal format).
  - `crates/meerkat-mobkit-core/src/runtime.rs` now emits concrete unauthorized reasons (`provider_mismatch`, `email_not_allowlisted`, `missing_credentials`, and service-identity denials), wires token auth at the console boundary (`auth_token`), and validates JWTs against trusted server-side OIDC/JWKS roots flowing from `RuntimeDecisionInputs -> RuntimeDecisionState` (no hardcoded trust roots).
  - `crates/meerkat-mobkit-core/src/auth.rs` adds local JWT validation (`validate_jwt_locally`) with HS256 signature checks and exp/nbf/iss/aud validation without IPC, plus OIDC/JWKS contract parsing and key selection (`parse_oidc_discovery_json`, `parse_jwks_json`, `select_jwk_for_token`).
  - `crates/meerkat-mobkit-core/src/rpc.rs` now provides `handle_console_ingress_json`, a non-test ingress caller that routes into `handle_console_rest_json_route`.
- Updated Phase 3c targets:
  - `choke_105_auth_to_console_route_target_defined_red` now validates real allow/deny middleware behavior and concrete error reasons.
  - `e2e_701_auth_flow_target_defined_red` now validates end-to-end protected-console auth flow via runtime token middleware using server-side trusted OIDC/JWKS, including user and service identity allowlist enforcement and adversarial query-anchor injection resistance.
  - `choke_110_sdk_contract_mapping_target_defined_red` preserves existing RPC error-shape assertions and adds SDK auth chokepoint coverage for local JWT validation path with no process-boundary dependency.
- Added Phase 7 tests:
  - `crates/meerkat-mobkit-core/tests/phase7.rs` covers provider support/default behavior, allowlist+service identity enforcement, local JWT validation, OIDC/JWKS contract parse+key selection, configured GitHub/GenericOidc allow paths, key-rotation behavior (trusted new key pass, stale key fail), runtime token middleware adversarial anchor-injection resistance, runtime trusted-auth config-flow behavior, and non-test ingress caller end-to-end auth routing.

## Phase 8 Remediation Evidence

- Updated console contract implementation:
  - `crates/meerkat-mobkit-core/src/runtime.rs` now exposes `/console/experience` with a concrete capability-driven contract for base panel + module panels and a unified activity-feed schema that anchors SSE/event-bus consumption to `mobkit/events/subscribe` (scopes, keep-alive contract, event envelope + SSE frame mapping).
  - `/console/experience` reuses the same Phase 7 auth middleware path as `/console/modules`, preserving protected-console behavior.
- Updated Phase 3c target:
  - `e2e_801_console_experience_target_defined_red` now asserts concrete console experience behavior instead of placeholder output.
- Added Phase 8 tests:
  - `crates/meerkat-mobkit-core/tests/phase8.rs` covers capability-driven rendering schema, unified activity-feed contract alignment against real `mobkit/events/subscribe` outputs, and auth-protected access enforcement on console routes.

## Phase 9 Remediation Evidence

- Scheduling runtime/module surface:
  - `crates/meerkat-mobkit-core/src/runtime.rs` now defines concrete schedule contract types (`schedule_id`, cron-like `interval`, `timezone`, `enabled`, `jitter_ms`, `catch_up`), deterministic tick evaluation (`evaluate_schedules_at_tick`) with deterministic jitter offset, explicit typed validation errors for malformed schedules (no silent drops), idempotent per-due-tick claim dispatch, bounded claim retention/pruning, bounded `scheduling_last_due_ticks` pruning, catch-up dispatch tracking, and scheduling supervisor signal injection into runtime dispatch events/results.
  - `crates/meerkat-mobkit-core/src/rpc.rs` now exposes `mobkit/scheduling/evaluate` and `mobkit/scheduling/dispatch` with strict typed param parsing (`enabled` must be boolean when present), explicit duplicate `schedule_id` rejection, and invalid interval/timezone JSON-RPC invalid-params error shapes.
- Updated Phase 3c targets:
  - `choke_106_scheduling_dispatch_handoff_target_defined_red` now asserts concrete same-tick idempotent claim behavior through `mobkit/scheduling/dispatch`.
  - `e2e_901_scheduled_action_flow_target_defined_red` now asserts cron-like evaluate->dispatch flow and replay visibility of scheduling dispatch events.
- Added Phase 9 tests:
  - `crates/meerkat-mobkit-core/tests/phase9.rs` covers timezone/interval evaluation behavior, explicit invalid-params behavior for malformed scheduling input, duplicate schedule-id rejection, bounded-claim pruning semantics, deterministic jitter behavior, catch-up dispatch semantics, idempotent claim semantics, and supervisor/restart wiring on scheduling dispatch path.
