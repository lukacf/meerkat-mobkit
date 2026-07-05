//! Reusable assembly for the FULL agent-memory stack (§6 architecture): the
//! bundled SQLite store with the §10.1 taint firewall, plus the judgment
//! plane (Distiller, Steward) and the member-event observer feeding them.
//!
//! Until this module existed the stack was hand-assembled inside
//! `rpc_gateway.rs`, which made the judgment plane unreachable for Rust
//! embedders driving `UnifiedRuntimeBuilder` directly (the OB3 deployment
//! shape: no gateways, no SDKs — builder or nothing). The builder now exposes
//! it through [`crate::UnifiedRuntimeBuilder::persistent_agent_memory_stack`].
//!
//! Scope notes, deliberate for v1:
//! - The **Hygienist** stays gateway-wired: it curates transcripts through
//!   the typed transcript-revision extension on the gateway's concrete
//!   session service, a seam the builder does not own yet.
//! - The gateway keeps its existing hand wiring (identical semantics, more
//!   seams: fail-init surfacing, gating/conflict bridges, schedule-host dream
//!   suppression). Converging it onto this module is tracked follow-up work.

use std::path::Path;
use std::sync::Arc;

use crate::MemberAgentEventSink;
use crate::identity_first::agent_memory::{AgentMemoryConfig, AgentMemoryProvider};
use crate::memory::distiller::{
    DistillerConfig, DistillerEngine, DistillerProfile, DistillerTriggers, FactoryDistillerHandle,
    HnswDiscardSource, SessionStoreTranscriptSource,
};
use crate::memory::events::MemoryEventSink;
use crate::memory::sqlite_store::SqliteAgentMemoryStore;
use crate::memory::steward::{
    FactoryStewardHandle, MemoryConflictBridge, MemoryGatingBridge, MobPurposeSource,
    SessionStoreEvidenceResolver, StewardConfig, StewardEngine, StewardProfile, StewardTriggers,
};
use crate::memory::taint::{SessionTaintTracker, TaintLlmWriteGate};

/// Engine enables + models for the judgment plane. `Default` = store +
/// firewall only (no background LLM work), matching the architecture's
/// "each stage ships alone" posture.
#[derive(Debug, Clone, Default)]
pub struct MemoryEnginesConfig {
    pub distiller: DistillerConfig,
    pub steward: StewardConfig,
}

/// Host-supplied seams for the stack: where the engines read transcripts,
/// where events project, and the steward's late-binding bridges. Everything
/// optional degrades gracefully (engines that need a missing seam fail the
/// build with a named error; bridges just don't bind).
#[derive(Default)]
pub struct MemoryStackSeams {
    pub persistent_state: Option<std::path::PathBuf>,
    pub transcript_store: Option<Arc<dyn meerkat::SessionStore>>,
    pub event_sink: Option<Arc<dyn MemoryEventSink>>,
    pub mob_purpose: Option<Arc<dyn MobPurposeSource>>,
    pub steward_gating: Option<Arc<dyn MemoryGatingBridge>>,
    pub steward_conflicts: Option<Arc<dyn MemoryConflictBridge>>,
}

/// The assembled stack. The caller finishes the wiring that needs the live
/// runtime: register `store` as the console panel store, pass `sinks` to
/// [`crate::spawn_member_event_observer`], hand `taint`/engines to the
/// injector, and start the steward dream (schedule host or
/// `StewardEngine::spawn_dream_loop`).
pub struct AgentMemoryStack {
    pub provider: Arc<dyn AgentMemoryProvider>,
    pub store: SqliteAgentMemoryStore,
    pub taint: SessionTaintTracker,
    pub distiller: Option<Arc<DistillerEngine>>,
    pub steward: Option<Arc<StewardEngine>>,
    /// Observe-stream consumers (taint + engine triggers), in wiring order.
    pub sinks: Vec<Arc<dyn MemberAgentEventSink>>,
}

/// Open the bundled SQLite store with the §10.1 firewall and assemble the
/// enabled engines. `persistent_state` is the runtime's state dir (engine
/// LLM factories and the compaction-discard source live under it);
/// `transcript_store` is a read handle on the session store the mob bridge
/// persists to (required when either engine is enabled).
pub fn build_sqlite_memory_stack(
    memory_dir: &Path,
    config: &AgentMemoryConfig,
    engines: &MemoryEnginesConfig,
    seams: MemoryStackSeams,
) -> Result<AgentMemoryStack, String> {
    let store = SqliteAgentMemoryStore::open(memory_dir)
        .map_err(|e| format!("failed to open agent memory store: {e}"))?;
    attach_memory_engines(store, config, engines, seams)
}

/// Stage-2 assembly over an already-open store: wire the §10.1 firewall
/// (tracker + write gate + event sinks) and the enabled engines. Used by the
/// builder path, where the store doubles as the provider handed to the
/// customizer before the runtime exists.
pub fn attach_memory_engines(
    store: SqliteAgentMemoryStore,
    config: &AgentMemoryConfig,
    engines: &MemoryEnginesConfig,
    seams: MemoryStackSeams,
) -> Result<AgentMemoryStack, String> {
    let MemoryStackSeams {
        persistent_state,
        transcript_store,
        event_sink,
        mob_purpose,
        steward_gating,
        steward_conflicts,
    } = seams;
    let persistent_state = persistent_state.as_deref();
    let event_sink =
        event_sink.ok_or_else(|| "agent memory stack requires an event sink".to_string())?;
    // §10.1 taint firewall: the Recorder must not ship without it. This
    // deliberately REPLACES any trackerless posture gate installed earlier.
    let taint = SessionTaintTracker::new(config.content_trust.clone());
    store.set_llm_write_gate(Arc::new(TaintLlmWriteGate::new(
        Some(taint.clone()),
        config.llm_writes,
    )));
    store.set_event_sink(event_sink.clone());
    taint.set_event_sink(event_sink.clone());

    let mut sinks: Vec<Arc<dyn MemberAgentEventSink>> = vec![Arc::new(taint.clone())];
    let realm = config.realm.clone();

    let state_for_engines = |what: &str| -> Result<&Path, String> {
        persistent_state.ok_or_else(|| format!("agent memory {what} requires persistent_state"))
    };
    let transcripts_for_engines = |what: &str| -> Result<Arc<dyn meerkat::SessionStore>, String> {
        transcript_store
            .clone()
            .ok_or_else(|| format!("agent memory {what} requires a session transcript store"))
    };

    let distiller = if engines.distiller.enabled {
        let state = state_for_engines("distiller")?;
        let transcripts = transcripts_for_engines("distiller")?;
        let mut profile = DistillerProfile::embedded_default();
        if let Some(model) = engines.distiller.model.as_deref() {
            profile = profile
                .with_model_override(model)
                .map_err(|e| format!("agent memory distiller: {e}"))?;
        }
        let handle =
            FactoryDistillerHandle::new(state, meerkat::Config::default(), &realm, &profile);
        let engine = Arc::new(DistillerEngine::new(
            profile,
            engines.distiller.clone(),
            Arc::new(handle),
            Arc::new(store.clone()),
            Arc::new(store.clone()),
            Arc::new(SessionStoreTranscriptSource::new(transcripts)),
            // Meerkat's session semantic memory lives at
            // <persistent_state>/memory; absent dir ⇒ nothing preserved.
            Some(Arc::new(HnswDiscardSource::new(state.join("memory")))),
            Some(taint.clone()),
            realm.clone(),
        ));
        engine.set_event_sink(event_sink.clone());
        sinks.push(Arc::new(DistillerTriggers::new(engine.clone())));
        Some(engine)
    } else {
        None
    };

    let steward = if engines.steward.enabled {
        let state = state_for_engines("steward")?;
        let transcripts = transcripts_for_engines("steward")?;
        let mut profile = StewardProfile::embedded_default();
        if let Some(model) = engines.steward.model.as_deref() {
            profile = profile
                .with_model_override(model)
                .map_err(|e| format!("agent memory steward: {e}"))?;
        }
        let transcripts_source: Arc<dyn crate::memory::distiller::TranscriptSource> =
            Arc::new(SessionStoreTranscriptSource::new(transcripts));
        // §10.2 P3 validator extension: agent_verified retiers must cite
        // evidence that resolves against the session store.
        store.set_evidence_resolver(Arc::new(SessionStoreEvidenceResolver::new(
            transcripts_source.clone(),
            tokio::runtime::Handle::current(),
        )));
        let handle = FactoryStewardHandle::new(
            state.to_path_buf(),
            meerkat::Config::default(),
            realm.clone(),
            &profile,
        );
        let mut engine = StewardEngine::new(
            profile,
            engines.steward.clone(),
            Arc::new(handle),
            Arc::new(store.clone()),
            transcripts_source,
            realm,
        )
        .with_events(event_sink.clone())
        .with_operator_routing(
            config.operator_scope
                == crate::identity_first::agent_memory::AgentMemoryOperatorScope::Provisional,
        );
        if let Some(purpose) = mob_purpose {
            engine = engine.with_mob_context(purpose);
        }
        if let Some(gating) = steward_gating {
            engine = engine.with_gating(gating);
        }
        if let Some(conflicts) = steward_conflicts {
            engine = engine.with_conflicts(conflicts);
        }
        let engine = Arc::new(engine);
        sinks.push(Arc::new(StewardTriggers::new(engine.clone())));
        Some(engine)
    } else {
        None
    };

    Ok(AgentMemoryStack {
        provider: Arc::new(store.clone()),
        store,
        taint,
        distiller,
        steward,
        sinks,
    })
}
