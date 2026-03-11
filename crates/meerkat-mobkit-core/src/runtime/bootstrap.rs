//! Runtime bootstrap — config resolution, module startup, and event loop initialization.

use super::*;

pub fn start_mobkit_runtime(
    config: MobKitConfig,
    agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Duration,
) -> Result<MobkitRuntimeHandle, MobkitRuntimeError> {
    start_mobkit_runtime_with_options(config, agent_events, timeout, RuntimeOptions::default())
}

pub fn start_mobkit_runtime_with_options(
    config: MobKitConfig,
    agent_events: Vec<EventEnvelope<UnifiedEvent>>,
    timeout: Duration,
    options: RuntimeOptions,
) -> Result<MobkitRuntimeHandle, MobkitRuntimeError> {
    let delivery_runtime_epoch_ms = current_time_ms();
    let mut lifecycle_events = Vec::new();
    let mut seq = 0_u64;
    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::MobStarted,
    });
    seq += 1;

    let mut supervisor_transitions = Vec::new();
    let mut module_events = Vec::new();
    let mut loaded_modules = BTreeSet::new();
    let mut live_children = BTreeMap::new();

    for module_id in &config.discovery.modules {
        let module = config
            .modules
            .iter()
            .find(|module| &module.id == module_id)
            .ok_or_else(|| {
                MobkitRuntimeError::Config(ConfigResolutionError::ModuleNotConfigured(
                    module_id.clone(),
                ))
            })?;

        let pre_spawn = config
            .pre_spawn
            .iter()
            .find(|data| data.module_id == *module_id);

        let mut start_result = supervise_module_start(module, pre_spawn, timeout, &options);
        supervisor_transitions.append(&mut start_result.transitions);
        if let Some(error) = start_result.terminal_error.as_ref() {
            let timestamp_ms = current_time_ms();
            module_events.push(EventEnvelope {
                event_id: format!("evt-supervisor-warning-{}-{timestamp_ms}", module.id),
                source: "module".to_string(),
                timestamp_ms,
                event: UnifiedEvent::Module(ModuleEvent {
                    module: module.id.clone(),
                    event_type: "supervisor.warning".to_string(),
                    payload: serde_json::json!({
                        "error": format!("{error:?}")
                    }),
                }),
            });
        }
        if let Some(event) = start_result.event {
            loaded_modules.insert(module_id.clone());
            if let Some(child) = start_result.child {
                live_children.insert(module_id.clone(), child);
            }
            module_events.push(event);
        }
    }

    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::ModulesStarted,
    });
    seq += 1;

    let merged_events = merge_unified_events(module_events, agent_events);
    lifecycle_events.push(LifecycleEvent {
        seq,
        stage: LifecycleStage::MergedStreamStarted,
    });

    let memory_backend = match options.memory_backend.as_ref() {
        Some(MemoryBackendConfig::Elephant(config)) => Some(
            ElephantMemoryStoreAdapter::from_config(config)
                .map_err(MobkitRuntimeError::MemoryBackend)?,
        ),
        None => None,
    };
    let persisted_memory = match memory_backend.as_ref() {
        Some(backend) => backend
            .read_state()
            .map_err(MobkitRuntimeError::MemoryBackend)?,
        None => PersistedMemoryState::default(),
    };
    let mut memory_assertions = persisted_memory
        .assertions
        .into_iter()
        .filter_map(|assertion| {
            let entity = MobkitRuntimeHandle::canonical_memory_token(&assertion.entity)?;
            let topic = MobkitRuntimeHandle::canonical_memory_token(&assertion.topic)?;
            let store = MobkitRuntimeHandle::canonical_memory_store(&assertion.store)?;
            let fact = assertion.fact.trim();
            if fact.is_empty() {
                return None;
            }
            Some(MemoryAssertion {
                assertion_id: assertion.assertion_id,
                entity,
                topic,
                store,
                fact: fact.to_string(),
                metadata: assertion.metadata,
                indexed_at_ms: assertion.indexed_at_ms,
            })
        })
        .collect::<Vec<_>>();
    while memory_assertions.len() > MEMORY_ASSERTIONS_MAX_RETAINED {
        memory_assertions.remove(0);
    }
    let mut memory_conflicts = BTreeMap::new();
    for signal in persisted_memory.conflicts {
        let Some(entity) = MobkitRuntimeHandle::canonical_memory_token(&signal.entity) else {
            continue;
        };
        let Some(topic) = MobkitRuntimeHandle::canonical_memory_token(&signal.topic) else {
            continue;
        };
        let Some(store) = MobkitRuntimeHandle::canonical_memory_store(&signal.store) else {
            continue;
        };
        let normalized_signal = MemoryConflictSignal {
            entity: entity.clone(),
            topic: topic.clone(),
            store: store.clone(),
            reason: signal
                .reason
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            updated_at_ms: signal.updated_at_ms,
        };
        memory_conflicts.insert(
            MemoryConflictKey {
                entity,
                topic,
                store,
            },
            normalized_signal,
        );
    }
    let memory_sequence = memory_assertions
        .iter()
        .filter_map(|assertion| parse_memory_assertion_sequence(&assertion.assertion_id))
        .max()
        .map(|last_sequence| last_sequence.saturating_add(1))
        .unwrap_or(memory_assertions.len() as u64);

    Ok(MobkitRuntimeHandle {
        config,
        runtime_options: options,
        loaded_modules,
        live_children,
        lifecycle_events,
        supervisor_report: SupervisorReport {
            transitions: supervisor_transitions,
        },
        merged_events,
        scheduling_claims: BTreeSet::new(),
        scheduling_claim_ticks: BTreeMap::new(),
        scheduling_last_due_ticks: BTreeMap::new(),
        scheduling_dispatch_sequence: 0,
        routing_sequence: 0,
        routing_resolutions: BTreeMap::new(),
        routing_resolution_order: Vec::new(),
        runtime_routes: BTreeMap::new(),
        delivery_sequence: 0,
        delivery_runtime_epoch_ms,
        delivery_now_floor_ms: 0,
        delivery_clock_ms: 0,
        delivery_history: Vec::new(),
        delivery_idempotency: BTreeMap::new(),
        delivery_idempotency_by_delivery: BTreeMap::new(),
        delivery_rate_window_counts: BTreeMap::new(),
        gating_sequence: 0,
        gating_pending: BTreeMap::new(),
        gating_pending_order: Vec::new(),
        gating_audit: Vec::new(),
        memory_sequence,
        memory_assertions,
        memory_conflicts,
        memory_backend,
        running: true,
    })
}

fn parse_memory_assertion_sequence(assertion_id: &str) -> Option<u64> {
    assertion_id
        .strip_prefix("memory-assert-")
        .and_then(|suffix| suffix.parse::<u64>().ok())
}
