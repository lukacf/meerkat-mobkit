#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]
use std::time::Duration;

use chrono::{TimeZone, Utc};
use meerkat_mobkit_core::rpc::MAX_SCHEDULES_PER_REQUEST;
use meerkat_mobkit_core::runtime::ScheduleValidationError;
use meerkat_mobkit_core::{
    DiscoverySpec, MobKitConfig, ModuleConfig, ModuleHealthState, PreSpawnData, RestartPolicy,
    RuntimeOptions, ScheduleDefinition, UnifiedEvent, evaluate_schedules_at_tick,
    handle_mobkit_rpc_json, start_mobkit_runtime, start_mobkit_runtime_with_options,
};
use serde_json::{Value, json};

fn shell_module(id: &str, script: &str, restart_policy: RestartPolicy) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        restart_policy,
    }
}

fn runtime_for_schedule_dispatch() -> meerkat_mobkit_core::MobkitRuntimeHandle {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "router",
            r#"printf '%s\n' '{"event_id":"evt-router","source":"module","timestamp_ms":10,"event":{"kind":"module","module":"router","event_type":"ready","payload":{"ok":true}}}'"#,
            RestartPolicy::Never,
        )],
        discovery: DiscoverySpec {
            namespace: "phase9".to_string(),
            modules: vec!["router".to_string()],
        },
        pre_spawn: vec![PreSpawnData {
            module_id: "router".to_string(),
            env: vec![],
        }],
    };

    start_mobkit_runtime(config, vec![], Duration::from_secs(1)).expect("runtime starts")
}

fn runtime_with_scheduling_supervisor_restart() -> meerkat_mobkit_core::MobkitRuntimeHandle {
    let config = MobKitConfig {
        modules: vec![shell_module(
            "scheduling",
            r#"printf '%s\n' '{"event_id":"evt-scheduling","source":"module","timestamp_ms":15,"event":{"kind":"module","module":"scheduling","event_type":"ready","payload":{"scheduler":true}}}'"#,
            RestartPolicy::Always,
        )],
        discovery: DiscoverySpec {
            namespace: "phase9".to_string(),
            modules: vec!["scheduling".to_string()],
        },
        pre_spawn: vec![PreSpawnData {
            module_id: "scheduling".to_string(),
            env: vec![],
        }],
    };

    start_mobkit_runtime_with_options(
        config,
        vec![],
        Duration::from_secs(1),
        RuntimeOptions {
            on_failure_retry_budget: 1,
            always_restart_budget: 1,
            ..RuntimeOptions::default()
        },
    )
    .expect("runtime starts")
}

fn parse_response(line: &str) -> Value {
    serde_json::from_str(line).expect("valid rpc response json")
}

fn utc_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> u64 {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("valid UTC datetime")
        .timestamp_millis() as u64
}

#[test]
fn phase9_schedule_001_timezone_and_interval_evaluation_is_deterministic() {
    let schedules = vec![
        ScheduleDefinition {
            schedule_id: "sched-c".to_string(),
            interval: "*/30m".to_string(),
            timezone: "UTC+01:00".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
        ScheduleDefinition {
            schedule_id: "sched-a".to_string(),
            interval: "*/1h".to_string(),
            timezone: "UTC".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
        ScheduleDefinition {
            schedule_id: "sched-b".to_string(),
            interval: "*/1h".to_string(),
            timezone: "UTC+01:00".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
        ScheduleDefinition {
            schedule_id: "sched-z".to_string(),
            interval: "*/1h".to_string(),
            timezone: "UTC-00:30".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
    ];

    let evaluation =
        evaluate_schedules_at_tick(&schedules, 3_600_000).expect("valid schedules should evaluate");
    let ids = evaluation
        .due_triggers
        .iter()
        .map(|trigger| trigger.schedule_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(evaluation.tick_ms, 3_600_000);
    assert_eq!(ids, vec!["sched-a", "sched-b", "sched-c"]);
}

#[test]
fn phase9_schedule_002_dispatch_claims_are_idempotent_per_tick() {
    let mut runtime = runtime_for_schedule_dispatch();
    let schedules = vec![ScheduleDefinition {
        schedule_id: "delivery-minute".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let first = runtime
        .dispatch_schedule_tick(&schedules, 120_000)
        .expect("dispatch succeeds");
    let second = runtime
        .dispatch_schedule_tick(&schedules, 120_000)
        .expect("dispatch succeeds");
    let third = runtime
        .dispatch_schedule_tick(&schedules, 180_000)
        .expect("dispatch succeeds");

    runtime.shutdown();

    assert_eq!(first.dispatched.len(), 1);
    assert_eq!(first.skipped_claims, Vec::<String>::new());
    assert_eq!(second.dispatched.len(), 0);
    assert_eq!(second.skipped_claims, vec!["delivery-minute:120000"]);
    assert_eq!(third.dispatched.len(), 1);
    assert_eq!(third.dispatched[0].claim_key, "delivery-minute:180000");
}

#[test]
fn phase9_req_002_choke_106_supervisor_restart_signal_is_wired_into_dispatch() {
    let mut runtime = runtime_with_scheduling_supervisor_restart();
    let schedules = vec![ScheduleDefinition {
        schedule_id: "sched-restart".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let dispatch = runtime
        .dispatch_schedule_tick(&schedules, 60_000)
        .expect("dispatch succeeds");
    let saw_restart_transition = runtime
        .supervisor_report()
        .transitions
        .iter()
        .any(|transition| {
            transition.module_id == "scheduling" && transition.to == ModuleHealthState::Restarting
        });
    let saw_supervisor_restart_event = runtime.merged_events().iter().any(|envelope| {
        matches!(
            &envelope.event,
            UnifiedEvent::Module(event)
                if event.module == "scheduling" && event.event_type == "supervisor.restart"
        )
    });

    runtime.shutdown();

    assert!(saw_restart_transition);
    assert_eq!(dispatch.dispatched.len(), 1);
    assert_eq!(dispatch.dispatched[0].schedule_id, "sched-restart");
    assert!(
        dispatch.dispatched[0]
            .supervisor_signal
            .as_ref()
            .expect("supervisor signal present")
            .restart_observed
    );
    assert!(saw_supervisor_restart_event);
}

#[test]
fn phase9_schedule_003_rpc_rejects_invalid_interval_timezone_and_duplicate_schedule_id() {
    let mut runtime = runtime_for_schedule_dispatch();
    let invalid_interval = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-invalid-interval","method":"mobkit/scheduling/dispatch","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"every-minute","timezone":"UTC","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    let invalid_cron = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-invalid-cron","method":"mobkit/scheduling/dispatch","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"0 9 * *","timezone":"UTC","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    let invalid_timezone = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-invalid-timezone","method":"mobkit/scheduling/evaluate","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"Mars/Phobos","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    let duplicate_schedule_id = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-duplicate-schedule","method":"mobkit/scheduling/dispatch","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"UTC","enabled":true},{"schedule_id":"delivery-minute","interval":"*/5m","timezone":"UTC+01:00","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    let invalid_enabled = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-invalid-enabled","method":"mobkit/scheduling/evaluate","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"UTC","enabled":"yes"}]}}"#,
        Duration::from_secs(1),
    ));

    runtime.shutdown();

    assert_eq!(invalid_interval["error"]["code"], json!(-32602));
    assert_eq!(
        invalid_interval["error"]["message"],
        json!("Invalid params: invalid interval 'every-minute' for schedule_id 'delivery-minute'")
    );
    assert_eq!(invalid_cron["error"]["code"], json!(-32602));
    assert_eq!(
        invalid_cron["error"]["message"],
        json!("Invalid params: invalid interval '0 9 * *' for schedule_id 'delivery-minute'")
    );
    assert_eq!(invalid_timezone["error"]["code"], json!(-32602));
    assert_eq!(
        invalid_timezone["error"]["message"],
        json!("Invalid params: invalid timezone 'Mars/Phobos' for schedule_id 'delivery-minute'")
    );
    assert_eq!(duplicate_schedule_id["error"]["code"], json!(-32602));
    assert_eq!(
        duplicate_schedule_id["error"]["message"],
        json!("Invalid params: duplicate schedule_id 'delivery-minute' is not allowed")
    );
    assert_eq!(invalid_enabled["error"]["code"], json!(-32602));
    assert_eq!(
        invalid_enabled["error"]["message"],
        json!("Invalid params: enabled must be a boolean")
    );
}

#[test]
fn phase9_schedule_003c_rpc_rejects_oversized_schedules_request() {
    let mut runtime = runtime_for_schedule_dispatch();
    let oversized_schedules = vec![
        json!({
            "schedule_id": "delivery-minute",
            "interval": "*/1m",
            "timezone": "UTC",
            "enabled": true
        });
        MAX_SCHEDULES_PER_REQUEST + 1
    ];
    let expected_message = format!(
        "Invalid params: schedules must contain at most {MAX_SCHEDULES_PER_REQUEST} entries"
    );

    let evaluate_response = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc": "2.0",
            "id": "phase9-oversized-evaluate",
            "method": "mobkit/scheduling/evaluate",
            "params": {
                "tick_ms": 120_000,
                "schedules": oversized_schedules,
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));

    let oversized_schedules = vec![
        json!({
            "schedule_id": "delivery-minute",
            "interval": "*/1m",
            "timezone": "UTC",
            "enabled": true
        });
        MAX_SCHEDULES_PER_REQUEST + 1
    ];
    let dispatch_response = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &json!({
            "jsonrpc": "2.0",
            "id": "phase9-oversized-dispatch",
            "method": "mobkit/scheduling/dispatch",
            "params": {
                "tick_ms": 120_000,
                "schedules": oversized_schedules,
            }
        })
        .to_string(),
        Duration::from_secs(1),
    ));

    runtime.shutdown();

    assert_eq!(evaluate_response["error"]["code"], json!(-32602));
    assert_eq!(
        evaluate_response["error"]["message"],
        json!(expected_message)
    );
    assert_eq!(dispatch_response["error"]["code"], json!(-32602));
    assert_eq!(
        dispatch_response["error"]["message"],
        json!(expected_message)
    );
}

#[test]
fn phase9_schedule_003b_rejects_overflowing_interval_marker() {
    let overflow_interval = "*/213503982335d";
    let invalid_schedules = vec![ScheduleDefinition {
        schedule_id: "overflow-minute".to_string(),
        interval: overflow_interval.to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let eval_err = evaluate_schedules_at_tick(&invalid_schedules, 120_000)
        .expect_err("overflow interval marker should error");
    assert_eq!(
        eval_err,
        ScheduleValidationError::InvalidInterval {
            schedule_id: "overflow-minute".to_string(),
            interval: overflow_interval.to_string(),
        }
    );

    let mut runtime = runtime_for_schedule_dispatch();
    let rpc_overflow_interval = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &format!(
            r#"{{"jsonrpc":"2.0","id":"phase9-overflow-interval","method":"mobkit/scheduling/dispatch","params":{{"tick_ms":120000,"schedules":[{{"schedule_id":"overflow-minute","interval":"{overflow_interval}","timezone":"UTC","enabled":true}}]}}}}"#
        ),
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(rpc_overflow_interval["error"]["code"], json!(-32602));
    assert_eq!(
        rpc_overflow_interval["error"]["message"],
        json!(format!(
            "Invalid params: invalid interval '{overflow_interval}' for schedule_id 'overflow-minute'"
        ))
    );
}

#[test]
fn phase9_schedule_004_claims_are_pruned_and_jitter_catch_up_are_deterministic() {
    let mut runtime = runtime_for_schedule_dispatch();
    let catch_up_schedule = ScheduleDefinition {
        schedule_id: "delivery-catch-up".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: true,
    };
    let jitter_schedule = ScheduleDefinition {
        schedule_id: "delivery-jitter".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 10_000,
        catch_up: false,
    };
    let prune_schedule = ScheduleDefinition {
        schedule_id: "delivery-prune".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };

    let first = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&catch_up_schedule), 120_000)
        .expect("dispatch succeeds");
    let second = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&catch_up_schedule), 121_000)
        .expect("dispatch succeeds");
    let third = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&catch_up_schedule), 181_000)
        .expect("dispatch succeeds");
    let prune_seed = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&prune_schedule), 120_000)
        .expect("dispatch succeeds");
    let pruning_trigger = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&prune_schedule), 86_580_000)
        .expect("dispatch succeeds");
    let pruned_replay = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&prune_schedule), 120_000)
        .expect("dispatch succeeds");

    let jitter_due_ticks = (60_000..120_000)
        .filter(|tick| {
            !evaluate_schedules_at_tick(std::slice::from_ref(&jitter_schedule), *tick)
                .expect("evaluation succeeds")
                .due_triggers
                .is_empty()
        })
        .collect::<Vec<_>>();
    let jitter_due_ticks_repeat = (60_000..120_000)
        .filter(|tick| {
            !evaluate_schedules_at_tick(std::slice::from_ref(&jitter_schedule), *tick)
                .expect("evaluation succeeds")
                .due_triggers
                .is_empty()
        })
        .collect::<Vec<_>>();

    runtime.shutdown();

    assert_eq!(first.dispatched.len(), 1);
    assert_eq!(second.dispatched.len(), 0);
    assert_eq!(third.dispatched.len(), 1);
    assert_eq!(third.dispatched[0].due_tick_ms, 180_000);
    assert_eq!(pruning_trigger.dispatched.len(), 1);
    assert_eq!(pruned_replay.dispatched.len(), 1);
    assert_eq!(
        pruned_replay.dispatched[0].claim_key,
        "delivery-prune:120000"
    );
    assert_ne!(
        prune_seed.dispatched[0].event_id,
        pruned_replay.dispatched[0].event_id
    );
    assert_eq!(jitter_due_ticks, jitter_due_ticks_repeat);
    assert_eq!(jitter_due_ticks.len(), 1);
    assert!(jitter_due_ticks[0] > 60_000);
}

#[test]
fn phase9_schedule_005_runtime_apis_reject_invalid_schedules_without_silent_drop() {
    let mut runtime = runtime_for_schedule_dispatch();
    let invalid_schedules = vec![ScheduleDefinition {
        schedule_id: "delivery-minute".to_string(),
        interval: "every-minute".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let eval_err = evaluate_schedules_at_tick(&invalid_schedules, 120_000)
        .expect_err("invalid interval should error");
    let dispatch_err = runtime
        .dispatch_schedule_tick(&invalid_schedules, 120_000)
        .expect_err("invalid interval should error");
    runtime.shutdown();

    assert_eq!(
        eval_err,
        ScheduleValidationError::InvalidInterval {
            schedule_id: "delivery-minute".to_string(),
            interval: "every-minute".to_string(),
        }
    );
    assert_eq!(dispatch_err, eval_err);
}

#[test]
fn phase9_schedule_007_runtime_and_rpc_reject_empty_schedule_id() {
    let mut runtime = runtime_for_schedule_dispatch();
    let invalid_schedules = vec![ScheduleDefinition {
        schedule_id: " \t".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let eval_err = evaluate_schedules_at_tick(&invalid_schedules, 120_000)
        .expect_err("empty schedule_id should error");
    let dispatch_err = runtime
        .dispatch_schedule_tick(&invalid_schedules, 120_000)
        .expect_err("empty schedule_id should error");
    let rpc_empty_schedule = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-empty-schedule-id","method":"mobkit/scheduling/evaluate","params":{"tick_ms":120000,"schedules":[{"schedule_id":"  ","interval":"*/1m","timezone":"UTC","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(eval_err, ScheduleValidationError::EmptyScheduleId);
    assert_eq!(dispatch_err, ScheduleValidationError::EmptyScheduleId);
    assert_eq!(rpc_empty_schedule["error"]["code"], json!(-32602));
    assert_eq!(
        rpc_empty_schedule["error"]["message"],
        json!("Invalid params: schedule_id must be a non-empty string")
    );
}

#[test]
fn phase9_schedule_007b_runtime_and_rpc_reject_empty_timezone() {
    let mut runtime = runtime_for_schedule_dispatch();
    let invalid_schedules = vec![ScheduleDefinition {
        schedule_id: "delivery-minute".to_string(),
        interval: "*/1m".to_string(),
        timezone: " \t".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let eval_err = evaluate_schedules_at_tick(&invalid_schedules, 120_000)
        .expect_err("empty timezone should error");
    let dispatch_err = runtime
        .dispatch_schedule_tick(&invalid_schedules, 120_000)
        .expect_err("empty timezone should error");
    let rpc_empty_timezone = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-empty-timezone","method":"mobkit/scheduling/evaluate","params":{"tick_ms":120000,"schedules":[{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"  ","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(
        eval_err,
        ScheduleValidationError::InvalidTimezone {
            schedule_id: "delivery-minute".to_string(),
            timezone: " \t".to_string(),
        }
    );
    assert_eq!(dispatch_err, eval_err);
    assert_eq!(rpc_empty_timezone["error"]["code"], json!(-32602));
    assert_eq!(
        rpc_empty_timezone["error"]["message"],
        json!("Invalid params: timezone must be a non-empty string")
    );
}

#[test]
fn phase9_schedule_006_last_due_ticks_are_pruned_like_claims() {
    let mut runtime = runtime_for_schedule_dispatch();
    let stale_schedule = ScheduleDefinition {
        schedule_id: "delivery-catch-up-prune".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: true,
    };
    let pruning_driver_schedule = ScheduleDefinition {
        schedule_id: "delivery-catch-up-prune-driver".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: true,
    };

    let seed = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&stale_schedule), 60_000)
        .expect("dispatch succeeds");
    let advanced = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&pruning_driver_schedule), 86_580_000)
        .expect("dispatch succeeds");
    let replay_after_prune = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&stale_schedule), 60_000)
        .expect("dispatch succeeds");

    runtime.shutdown();

    assert_eq!(advanced.dispatched.len(), 1);
    assert_eq!(advanced.dispatched[0].due_tick_ms, 86_580_000);
    assert_eq!(replay_after_prune.dispatched.len(), 1);
    assert_eq!(replay_after_prune.dispatched[0].due_tick_ms, 60_000);
    assert_eq!(
        replay_after_prune.dispatched[0].claim_key,
        "delivery-catch-up-prune:60000"
    );
    assert_ne!(
        seed.dispatched[0].event_id,
        replay_after_prune.dispatched[0].event_id
    );
}

#[test]
fn phase9_schedule_008_cron_and_iana_timezones_are_supported_with_dst_awareness() {
    let cron_schedule = ScheduleDefinition {
        schedule_id: "la-cron".to_string(),
        interval: "0 9 * * *".to_string(),
        timezone: "America/Los_Angeles".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let fixed_offset_schedule = ScheduleDefinition {
        schedule_id: "la-fixed".to_string(),
        interval: "0 9 * * *".to_string(),
        timezone: "UTC-08:00".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let winter_due_utc = utc_ms(2024, 1, 15, 17, 0, 0);
    let summer_due_utc = utc_ms(2024, 7, 15, 16, 0, 0);

    let winter_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&cron_schedule), winter_due_utc)
            .expect("winter cron eval succeeds");
    let summer_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&cron_schedule), summer_due_utc)
            .expect("summer cron eval succeeds");
    let fixed_winter_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&fixed_offset_schedule), winter_due_utc)
            .expect("fixed offset winter eval succeeds");
    let fixed_summer_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&fixed_offset_schedule), summer_due_utc)
            .expect("fixed offset summer eval succeeds");

    let invalid_cron = evaluate_schedules_at_tick(
        &[ScheduleDefinition {
            interval: "0 9 * *".to_string(),
            ..cron_schedule.clone()
        }],
        winter_due_utc,
    )
    .expect_err("unsupported cron should be rejected");

    assert_eq!(winter_eval.due_triggers.len(), 1);
    assert_eq!(summer_eval.due_triggers.len(), 1);
    assert_eq!(fixed_winter_eval.due_triggers.len(), 1);
    assert_eq!(fixed_summer_eval.due_triggers.len(), 0);
    assert_eq!(
        invalid_cron,
        ScheduleValidationError::InvalidInterval {
            schedule_id: "la-cron".to_string(),
            interval: "0 9 * *".to_string(),
        }
    );
}

#[test]
fn phase9_schedule_009_pruning_runs_even_when_dispatch_early_exits() {
    let mut runtime = runtime_for_schedule_dispatch();
    let old_claim_schedule = ScheduleDefinition {
        schedule_id: "old-claim".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let catch_up_skip_schedule = ScheduleDefinition {
        schedule_id: "catch-up-skip".to_string(),
        interval: "*/1d".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: true,
    };

    let seed = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&old_claim_schedule), 120_000)
        .expect("seed dispatch succeeds");
    let _catch_up_seed = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&catch_up_skip_schedule), 86_400_000)
        .expect("catch-up seed succeeds");
    let early_exit = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&catch_up_skip_schedule), 86_580_000)
        .expect("catch-up skip succeeds");
    let replay = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&old_claim_schedule), 120_000)
        .expect("replay dispatch succeeds");

    runtime.shutdown();

    assert_eq!(early_exit.dispatched.len(), 0);
    assert_eq!(early_exit.due_count, 0);
    assert_eq!(replay.dispatched.len(), 1);
    assert_eq!(replay.dispatched[0].claim_key, "old-claim:120000");
    assert_ne!(seed.dispatched[0].event_id, replay.dispatched[0].event_id);
}

#[test]
fn phase9_schedule_010_jitter_without_catch_up_dispatches_under_coarse_polling() {
    let schedule = ScheduleDefinition {
        schedule_id: "coarse-jitter".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 10_000,
        catch_up: false,
    };
    let due_tick = (60_000..120_000)
        .find(|tick| {
            !evaluate_schedules_at_tick(std::slice::from_ref(&schedule), *tick)
                .expect("evaluation succeeds")
                .due_triggers
                .is_empty()
        })
        .expect("exact due tick exists");
    let coarse_poll_tick = due_tick + 500;

    let mut runtime = runtime_for_schedule_dispatch();
    let first = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), coarse_poll_tick)
        .expect("first coarse dispatch succeeds");
    let second = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), coarse_poll_tick + 500)
        .expect("second coarse dispatch succeeds");
    runtime.shutdown();

    let mut runtime_repeat = runtime_for_schedule_dispatch();
    let first_repeat = runtime_repeat
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), coarse_poll_tick)
        .expect("repeat coarse dispatch succeeds");
    runtime_repeat.shutdown();

    assert_eq!(first.dispatched.len(), 1);
    assert_eq!(first.dispatched[0].due_tick_ms, due_tick);
    assert_eq!(second.dispatched.len(), 0);
    assert_eq!(second.due_count, 0);
    assert_eq!(first_repeat.dispatched.len(), 1);
    assert_eq!(first_repeat.dispatched[0].due_tick_ms, due_tick);
}

#[test]
fn phase9_schedule_011_cron_dow_range_ending_with_7_is_valid() {
    let schedule = ScheduleDefinition {
        schedule_id: "weekend-run".to_string(),
        interval: "0 9 * * 5-7".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let friday_tick = utc_ms(2024, 1, 5, 9, 0, 0);
    let sunday_tick = utc_ms(2024, 1, 7, 9, 0, 0);
    let monday_tick = utc_ms(2024, 1, 8, 9, 0, 0);

    let friday_eval = evaluate_schedules_at_tick(std::slice::from_ref(&schedule), friday_tick)
        .expect("friday should evaluate");
    let sunday_eval = evaluate_schedules_at_tick(std::slice::from_ref(&schedule), sunday_tick)
        .expect("sunday should evaluate");
    let monday_eval = evaluate_schedules_at_tick(std::slice::from_ref(&schedule), monday_tick)
        .expect("monday should evaluate");

    assert_eq!(friday_eval.due_triggers.len(), 1);
    assert_eq!(sunday_eval.due_triggers.len(), 1);
    assert_eq!(monday_eval.due_triggers.len(), 0);
}

#[test]
fn phase9_schedule_011b_cron_dow_step_wildcard_equivalent_preserves_dom_wildcard_semantics() {
    let schedule = ScheduleDefinition {
        schedule_id: "dom-with-dow-step-any".to_string(),
        interval: "0 9 15 * */1".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let matching_dom_tick = utc_ms(2024, 1, 15, 9, 0, 0);
    let non_matching_dom_tick = utc_ms(2024, 1, 16, 9, 0, 0);

    let matching_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&schedule), matching_dom_tick)
            .expect("matching dom should evaluate");
    let non_matching_eval =
        evaluate_schedules_at_tick(std::slice::from_ref(&schedule), non_matching_dom_tick)
            .expect("non-matching dom should evaluate");

    assert_eq!(matching_eval.due_triggers.len(), 1);
    assert_eq!(non_matching_eval.due_triggers.len(), 0);
}

#[test]
fn phase9_schedule_012_rpc_rejects_unsupported_tick_ms_for_evaluate_and_dispatch() {
    let mut runtime = runtime_for_schedule_dispatch();
    let unsupported_tick_ms = u64::MAX;

    let evaluate = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &format!(
            r#"{{"jsonrpc":"2.0","id":"phase9-unsupported-tick-evaluate","method":"mobkit/scheduling/evaluate","params":{{"tick_ms":{unsupported_tick_ms},"schedules":[{{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"UTC","enabled":true}}]}}}}"#
        ),
        Duration::from_secs(1),
    ));
    let dispatch = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        &format!(
            r#"{{"jsonrpc":"2.0","id":"phase9-unsupported-tick-dispatch","method":"mobkit/scheduling/dispatch","params":{{"tick_ms":{unsupported_tick_ms},"schedules":[{{"schedule_id":"delivery-minute","interval":"*/1m","timezone":"UTC","enabled":true}}]}}}}"#
        ),
        Duration::from_secs(1),
    ));

    runtime.shutdown();

    assert_eq!(evaluate["error"]["code"], json!(-32602));
    assert_eq!(
        evaluate["error"]["message"],
        json!(format!("Invalid params: tick_ms must be <= {}", i64::MAX))
    );
    assert_eq!(dispatch["error"]["code"], json!(-32602));
    assert_eq!(
        dispatch["error"]["message"],
        json!(format!("Invalid params: tick_ms must be <= {}", i64::MAX))
    );
}

#[test]
fn phase9_schedule_013_marker_intervals_with_iana_zone_are_dst_correct() {
    let mut runtime = runtime_for_schedule_dispatch();
    let schedule = ScheduleDefinition {
        schedule_id: "la-daily".to_string(),
        interval: "*/1d".to_string(),
        timezone: "America/Los_Angeles".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    // 2024-03-10 12:00:00Z is after LA's DST shift. Latest local midnight that day is 08:00:00Z.
    let tick_ms = utc_ms(2024, 3, 10, 12, 0, 0);
    let expected_due_tick_ms = utc_ms(2024, 3, 10, 8, 0, 0);

    let dispatch = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), tick_ms)
        .expect("dispatch succeeds");
    runtime.shutdown();

    assert_eq!(dispatch.dispatched.len(), 1);
    assert_eq!(dispatch.dispatched[0].due_tick_ms, expected_due_tick_ms);
}

#[test]
fn phase9_schedule_014_schedule_id_is_trimmed_for_duplicate_checks_and_identity() {
    let mut runtime = runtime_for_schedule_dispatch();
    let duplicate_schedule_id = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-duplicate-trimmed-schedule","method":"mobkit/scheduling/dispatch","params":{"tick_ms":120000,"schedules":[{"schedule_id":" delivery-minute ","interval":"*/1m","timezone":"UTC","enabled":true},{"schedule_id":"delivery-minute","interval":"*/5m","timezone":"UTC+01:00","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(duplicate_schedule_id["error"]["code"], json!(-32602));
    assert_eq!(
        duplicate_schedule_id["error"]["message"],
        json!("Invalid params: duplicate schedule_id 'delivery-minute' is not allowed")
    );

    let mut runtime = runtime_for_schedule_dispatch();
    let schedule = ScheduleDefinition {
        schedule_id: " delivery-minute ".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let first = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), 120_000)
        .expect("dispatch succeeds");
    let second = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), 120_000)
        .expect("dispatch succeeds");
    runtime.shutdown();

    assert_eq!(first.dispatched[0].schedule_id, "delivery-minute");
    assert_eq!(first.dispatched[0].claim_key, "delivery-minute:120000");
    assert_eq!(second.skipped_claims, vec!["delivery-minute:120000"]);
}

#[test]
fn phase9_schedule_015_supervisor_restart_event_emitted_once_per_dispatch_tick() {
    let mut runtime = runtime_with_scheduling_supervisor_restart();
    let schedules = vec![
        ScheduleDefinition {
            schedule_id: "sched-restart-a".to_string(),
            interval: "*/1m".to_string(),
            timezone: "UTC".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
        ScheduleDefinition {
            schedule_id: "sched-restart-b".to_string(),
            interval: "*/1m".to_string(),
            timezone: "UTC".to_string(),
            enabled: true,
            jitter_ms: 0,
            catch_up: false,
        },
    ];

    let dispatch = runtime
        .dispatch_schedule_tick(&schedules, 60_000)
        .expect("dispatch succeeds");
    let restart_events_for_tick = runtime
        .merged_events()
        .iter()
        .filter(|envelope| {
            envelope.timestamp_ms == 60_000
                && matches!(
                    &envelope.event,
                    UnifiedEvent::Module(event)
                        if event.module == "scheduling" && event.event_type == "supervisor.restart"
                )
        })
        .count();
    runtime.shutdown();

    assert_eq!(dispatch.dispatched.len(), 2);
    assert_eq!(restart_events_for_tick, 1);
}

#[test]
fn phase9_schedule_016_dispatch_event_append_preserves_merged_event_order() {
    let mut runtime = runtime_for_schedule_dispatch();
    let schedule = ScheduleDefinition {
        schedule_id: "event-order".to_string(),
        interval: "*/1m".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: true,
    };

    runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), 120_000)
        .expect("dispatch at 120000 succeeds");
    runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), 240_000)
        .expect("dispatch at 240000 succeeds");
    runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), 60_000)
        .expect("dispatch at 60000 succeeds");

    let merged = runtime.merged_events().to_vec();
    runtime.shutdown();

    assert!(merged.windows(2).all(|pair| {
        let left = &pair[0];
        let right = &pair[1];
        left.timestamp_ms < right.timestamp_ms
            || (left.timestamp_ms == right.timestamp_ms
                && (left.event_id < right.event_id
                    || (left.event_id == right.event_id && left.source <= right.source)))
    }));
}

#[test]
fn phase9_schedule_017_leap_day_cron_dispatches_under_multi_year_coarse_polling() {
    let mut runtime = runtime_for_schedule_dispatch();
    let schedule = ScheduleDefinition {
        schedule_id: "leap-day-cron".to_string(),
        interval: "0 0 29 2 *".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };

    let first_coarse_tick = utc_ms(2025, 3, 1, 0, 0, 0);
    let next_leap_coarse_tick = utc_ms(2028, 3, 1, 0, 0, 0);

    let first = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), first_coarse_tick)
        .expect("first coarse dispatch succeeds");
    let second = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), first_coarse_tick)
        .expect("same coarse tick dispatch succeeds");
    let third = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), next_leap_coarse_tick)
        .expect("next leap coarse dispatch succeeds");
    runtime.shutdown();

    assert_eq!(first.dispatched.len(), 1);
    assert_eq!(
        first.dispatched[0].due_tick_ms,
        utc_ms(2024, 2, 29, 0, 0, 0)
    );
    assert_eq!(
        first.dispatched[0].claim_key,
        format!("leap-day-cron:{}", utc_ms(2024, 2, 29, 0, 0, 0))
    );

    assert_eq!(second.dispatched.len(), 0);
    assert_eq!(
        second.skipped_claims,
        vec![format!("leap-day-cron:{}", utc_ms(2024, 2, 29, 0, 0, 0))]
    );

    assert_eq!(third.dispatched.len(), 1);
    assert_eq!(
        third.dispatched[0].due_tick_ms,
        utc_ms(2028, 2, 29, 0, 0, 0)
    );
    assert_eq!(
        third.dispatched[0].claim_key,
        format!("leap-day-cron:{}", utc_ms(2028, 2, 29, 0, 0, 0))
    );
}

#[test]
fn phase9_schedule_018_sparse_cron_same_tick_retry_is_idempotent() {
    let mut runtime = runtime_for_schedule_dispatch();
    let schedule = ScheduleDefinition {
        schedule_id: "sparse-feb29".to_string(),
        interval: "0 0 29 2 *".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    };
    let coarse_tick = utc_ms(2025, 3, 1, 0, 0, 0);
    let expected_due = utc_ms(2024, 2, 29, 0, 0, 0);
    let expected_claim = format!("sparse-feb29:{expected_due}");

    let first = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), coarse_tick)
        .expect("first coarse dispatch succeeds");
    let retry = runtime
        .dispatch_schedule_tick(std::slice::from_ref(&schedule), coarse_tick)
        .expect("same coarse tick retry succeeds");
    runtime.shutdown();

    assert_eq!(first.dispatched.len(), 1);
    assert_eq!(first.dispatched[0].due_tick_ms, expected_due);
    assert_eq!(first.dispatched[0].claim_key, expected_claim);

    assert_eq!(retry.dispatched.len(), 0);
    assert_eq!(
        retry.skipped_claims,
        vec![format!("sparse-feb29:{expected_due}")]
    );
}

#[test]
fn phase9_schedule_019_runtime_rejects_semantically_impossible_cron_interval() {
    let mut runtime = runtime_for_schedule_dispatch();
    let invalid_schedules = vec![ScheduleDefinition {
        schedule_id: "impossible-feb31".to_string(),
        interval: "0 0 31 2 *".to_string(),
        timezone: "UTC".to_string(),
        enabled: true,
        jitter_ms: 0,
        catch_up: false,
    }];

    let eval_err = evaluate_schedules_at_tick(&invalid_schedules, 120_000)
        .expect_err("impossible cron should be rejected");
    let dispatch_err = runtime
        .dispatch_schedule_tick(&invalid_schedules, 120_000)
        .expect_err("impossible cron should be rejected");
    runtime.shutdown();

    assert_eq!(
        eval_err,
        ScheduleValidationError::InvalidInterval {
            schedule_id: "impossible-feb31".to_string(),
            interval: "0 0 31 2 *".to_string(),
        }
    );
    assert_eq!(dispatch_err, eval_err);
}

#[test]
fn phase9_schedule_020_rpc_rejects_semantically_impossible_cron_interval() {
    let mut runtime = runtime_for_schedule_dispatch();
    let evaluate = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-impossible-cron-evaluate","method":"mobkit/scheduling/evaluate","params":{"tick_ms":120000,"schedules":[{"schedule_id":"impossible-feb31","interval":"0 0 31 2 *","timezone":"UTC","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    let dispatch = parse_response(&handle_mobkit_rpc_json(
        &mut runtime,
        r#"{"jsonrpc":"2.0","id":"phase9-impossible-cron-dispatch","method":"mobkit/scheduling/dispatch","params":{"tick_ms":120000,"schedules":[{"schedule_id":"impossible-feb31","interval":"0 0 31 2 *","timezone":"UTC","enabled":true}]}}"#,
        Duration::from_secs(1),
    ));
    runtime.shutdown();

    assert_eq!(evaluate["error"]["code"], json!(-32602));
    assert_eq!(
        evaluate["error"]["message"],
        json!("Invalid params: invalid interval '0 0 31 2 *' for schedule_id 'impossible-feb31'")
    );
    assert_eq!(dispatch["error"]["code"], json!(-32602));
    assert_eq!(
        dispatch["error"]["message"],
        json!("Invalid params: invalid interval '0 0 31 2 *' for schedule_id 'impossible-feb31'")
    );
}
