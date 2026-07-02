//! LLM Selector — recall judgment without a horizon
//! (docs/design/agent-memory-architecture.md §8.3).
//!
//! A bounded one-shot structured call: manifest + incoming turn text +
//! suppression list in, `{selected_ids, coverage}` out, under a versioned
//! calibration profile (§11). The selector is a side-query over the store's
//! manifest tiers, never over the injected index, and it is the only
//! judgment stage allowed on the turn path — everything else in this module
//! is deterministic plumbing around that single call.
//!
//! The model client comes host-side through meerkat's factory seam
//! `AgentFactory::build_llm_client_for_identity` (§8.1) via
//! [`FactorySelectorHandle`]; the coordinator reaches the stage through the
//! process-wide install ([`install`]/[`installed`]) because the memory
//! surfaces in `identity_first::agent_memory` construct their coordinators
//! internally and stay untouched by this stage.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use async_trait::async_trait;
use futures::StreamExt;
use rand_core::{OsRng, RngCore};
use serde::Deserialize;

use meerkat_client::{FactoryError, LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::{Message, Provider, SessionLlmIdentity, UserMessage};

use crate::identity_first::agent_memory::{
    AgentMemoryError, AgentMemoryRecord, compact_whitespace, truncate_utf8_boundary,
};
use crate::memory::records::{MemoryScope, RecordMeta};

/// Operator switch for the selector stage. Off by default: the model/auth
/// binding choice belongs to the operator (§8.1 open question 3); flipping
/// the default is a calibration-scorecard decision.
pub const SELECTOR_ENV_VAR: &str = "MOBKIT_AGENT_MEMORY_SELECTOR";

/// Embedded prompt bundle (crate-local copy of
/// `memory-evals/prompts/selector-v0.md`; a unit test enforces byte
/// equality so the calibration artifact and the shipped default cannot
/// drift).
pub const EMBEDDED_PROMPT_V0: &str = include_str!("selector_prompt_v0.md");

const MANIFEST_PLACEHOLDER: &str = "{{manifest}}";
const TURN_TEXT_PLACEHOLDER: &str = "{{turn_text}}";
const SUPPRESSION_PLACEHOLDER: &str = "{{suppression_list}}";

/// Bound the turn text rendered into the selector prompt; the manifest is
/// the judgment surface, the turn is context.
const MAX_PROMPT_TURN_BYTES: usize = 8 * 1024;
/// Output budget for the structured selection object.
const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 1024;
/// §8.3 scale posture: Full-tier sweeps chunk the description manifest into
/// side-model calls of roughly this many rendered bytes.
pub const FULL_SWEEP_CHUNK_BYTES: usize = 100 * 1024;
/// §8.3 scale-posture ceilings. Above the soft ceiling, Full-tier selection
/// is chunked — correct but slower and costlier, and it says so loudly. At
/// the hard ceiling (4× soft), the manifest truncates oldest-least-used
/// with a loud event naming what was dropped, and the scope needs steward
/// retention pressure. Defaults per §8.3; final numbers are a §16 question.
pub const FULL_SWEEP_SOFT_CEILING_RECORDS: usize = 4_000;
pub const FULL_SWEEP_HARD_CEILING_RECORDS: usize = 4 * FULL_SWEEP_SOFT_CEILING_RECORDS;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SelectorError {
    /// Calibration profile failed to load or validate (fail-loud).
    Profile(String),
    /// Client construction / auth resolution failed.
    Auth(String),
    /// The provider call itself failed.
    Client(String),
    /// The model's output never became valid structured JSON.
    Parse(String),
}

impl std::fmt::Display for SelectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Profile(msg) => write!(f, "selector profile error: {msg}"),
            Self::Auth(msg) => write!(f, "selector auth error: {msg}"),
            Self::Client(msg) => write!(f, "selector client error: {msg}"),
            Self::Parse(msg) => write!(f, "selector parse error: {msg}"),
        }
    }
}

impl std::error::Error for SelectorError {}

// ---------------------------------------------------------------------------
// Calibration profile (§11)
// ---------------------------------------------------------------------------

/// Params table of a selector calibration profile. Field set mirrors
/// `memory-evals/profiles/selector-v0.toml`; keep the two in sync.
#[derive(Debug, Clone, Deserialize)]
pub struct SelectorParams {
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_selection_bar")]
    pub selection_bar: String,
    #[serde(default = "default_shuffle_manifest")]
    pub shuffle_manifest: bool,
    #[serde(default = "default_recall_timeout_ms")]
    pub recall_timeout_ms: u64,
    /// K for the WorkingSet manifest tier the per-turn side-query reads.
    #[serde(default = "default_working_set_k")]
    pub working_set_k: usize,
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
}

fn default_temperature() -> f32 {
    0.0
}
fn default_selection_bar() -> String {
    "certain-to-be-helpful".to_string()
}
fn default_shuffle_manifest() -> bool {
    true
}
fn default_recall_timeout_ms() -> u64 {
    500
}
fn default_working_set_k() -> usize {
    200
}
fn default_max_output_tokens() -> u32 {
    DEFAULT_MAX_OUTPUT_TOKENS
}

impl Default for SelectorParams {
    fn default() -> Self {
        Self {
            temperature: default_temperature(),
            selection_bar: default_selection_bar(),
            shuffle_manifest: default_shuffle_manifest(),
            recall_timeout_ms: default_recall_timeout_ms(),
            working_set_k: default_working_set_k(),
            max_output_tokens: default_max_output_tokens(),
        }
    }
}

/// A loaded selector calibration profile: `{stage, version, model, prompt
/// bundle, params}` (§11), with the prompt template resolved to text.
#[derive(Debug, Clone)]
pub struct SelectorProfile {
    pub stage: String,
    pub version: String,
    pub model: String,
    pub provider: Provider,
    /// Repo-relative bundle path for provenance (`CalibrationRef`).
    pub prompt_bundle: String,
    pub prompt_template: String,
    pub params: SelectorParams,
}

#[derive(Debug, Deserialize)]
struct RawProfile {
    stage: String,
    version: String,
    model: String,
    /// Optional explicit provider; defaults to catalog inference on `model`.
    #[serde(default)]
    provider: Option<String>,
    prompt_bundle: String,
    #[serde(default)]
    params: Option<SelectorParams>,
}

impl SelectorProfile {
    /// The embedded default profile: `memory-evals/profiles/selector-v0.toml`
    /// with the prompt bundle compiled in, so the gateway needs no
    /// filesystem coupling.
    pub fn embedded_default() -> Self {
        Self {
            stage: "selector".to_string(),
            version: "0".to_string(),
            model: "claude-haiku-4-5".to_string(),
            provider: Provider::Anthropic,
            prompt_bundle: "prompts/selector-v0.md".to_string(),
            prompt_template: EMBEDDED_PROMPT_V0.to_string(),
            params: SelectorParams::default(),
        }
    }

    /// Load an external calibration profile (fail-loud): TOML in the §11
    /// format, prompt bundle resolved relative to the profile's directory
    /// first, then to its parent (the `memory-evals/` layout, where bundles
    /// are referenced relative to the evals root rather than `profiles/`).
    pub fn load(path: &Path) -> Result<Self, SelectorError> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            SelectorError::Profile(format!("cannot read profile '{}': {err}", path.display()))
        })?;
        let raw: RawProfile = toml::from_str(&text).map_err(|err| {
            SelectorError::Profile(format!("invalid profile '{}': {err}", path.display()))
        })?;
        if raw.stage != "selector" {
            return Err(SelectorError::Profile(format!(
                "profile '{}' is for stage '{}', not 'selector'",
                path.display(),
                raw.stage
            )));
        }
        if raw.model.trim().is_empty() || raw.model == "PLACEHOLDER" {
            return Err(SelectorError::Profile(format!(
                "profile '{}' does not name a model",
                path.display()
            )));
        }
        let provider = match raw.provider.as_deref() {
            Some(name) => Provider::parse_strict(name).ok_or_else(|| {
                SelectorError::Profile(format!(
                    "profile '{}': unknown provider '{name}'",
                    path.display()
                ))
            })?,
            None => meerkat_models::infer_provider(&raw.model).ok_or_else(|| {
                SelectorError::Profile(format!(
                    "profile '{}': model '{}' is not in the catalog; set `provider` explicitly",
                    path.display(),
                    raw.model
                ))
            })?,
        };
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let candidates = [
            base.join(&raw.prompt_bundle),
            base.parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&raw.prompt_bundle),
        ];
        let bundle_path = candidates.iter().find(|p| p.is_file()).ok_or_else(|| {
            SelectorError::Profile(format!(
                "profile '{}': prompt_bundle '{}' does not resolve",
                path.display(),
                raw.prompt_bundle
            ))
        })?;
        let prompt_template = std::fs::read_to_string(bundle_path).map_err(|err| {
            SelectorError::Profile(format!(
                "cannot read prompt bundle '{}': {err}",
                bundle_path.display()
            ))
        })?;
        let profile = Self {
            stage: raw.stage,
            version: raw.version,
            model: raw.model,
            provider,
            prompt_bundle: raw.prompt_bundle,
            prompt_template,
            params: raw.params.unwrap_or_default(),
        };
        profile.validate()?;
        Ok(profile)
    }

    fn validate(&self) -> Result<(), SelectorError> {
        for placeholder in [
            MANIFEST_PLACEHOLDER,
            TURN_TEXT_PLACEHOLDER,
            SUPPRESSION_PLACEHOLDER,
        ] {
            if !self.prompt_template.contains(placeholder) {
                return Err(SelectorError::Profile(format!(
                    "prompt bundle '{}' is missing placeholder `{placeholder}`",
                    self.prompt_bundle
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Selection output
// ---------------------------------------------------------------------------

/// Coverage verdict from the selector's structured output (§8.3): whether
/// the manifest slice it judged was enough, or a deeper full-store sweep
/// should run off the blocking path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Sufficient,
    NeedDeeperSweep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub selected_ids: Vec<String>,
    pub coverage: Coverage,
}

#[derive(Deserialize)]
struct RawSelection {
    selected_ids: Vec<String>,
    coverage: RawCoverage,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawCoverage {
    Sufficient,
    NeedDeeperSweep,
}

// ---------------------------------------------------------------------------
// The selection call
// ---------------------------------------------------------------------------

/// One selector invocation: render the profile's prompt bundle over the
/// manifest (position-shuffled per call — §8.3 bias guard), the incoming
/// turn text, and the suppression list; parse the structured
/// `{selected_ids, coverage}` reply. Unknown ids are dropped with a warning;
/// suppressed ids are never returned; a single JSON-repair round-trip runs
/// on parse failure, then the call errors.
pub async fn select(
    manifest: &[RecordMeta],
    turn_text: &str,
    suppressed_ids: &HashSet<String>,
    profile: &SelectorProfile,
    client: &dyn LlmClient,
) -> Result<Selection, SelectorError> {
    let prompt = render_prompt(profile, manifest, turn_text, suppressed_ids);
    let reply = complete_text(client, profile, prompt).await?;
    let raw = match parse_selection(&reply) {
        Ok(raw) => raw,
        Err(first_err) => {
            // One repair round-trip: hand the malformed reply back and ask
            // for exactly the JSON object, nothing else.
            let repair_prompt = format!(
                "The following reply was supposed to be exactly one JSON object of the form \
                 {{\"selected_ids\": [\"...\"], \"coverage\": \"sufficient\" | \"need_deeper_sweep\"}} \
                 but did not parse ({first_err}). Reply with ONLY the corrected JSON object, \
                 no other text.\n\n{reply}"
            );
            let repaired = complete_text(client, profile, repair_prompt).await?;
            parse_selection(&repaired).map_err(SelectorError::Parse)?
        }
    };
    let known: HashSet<&str> = manifest.iter().map(|meta| meta.id.as_str()).collect();
    let mut seen = HashSet::new();
    let mut selected_ids = Vec::new();
    for id in raw.selected_ids {
        if !known.contains(id.as_str()) {
            tracing::warn!(id = %id, "selector returned an id not present in the manifest; dropped");
            continue;
        }
        if suppressed_ids.contains(&id) {
            tracing::warn!(id = %id, "selector returned a suppressed id; dropped");
            continue;
        }
        if seen.insert(id.clone()) {
            selected_ids.push(id);
        }
    }
    Ok(Selection {
        selected_ids,
        coverage: match raw.coverage {
            RawCoverage::Sufficient => Coverage::Sufficient,
            RawCoverage::NeedDeeperSweep => Coverage::NeedDeeperSweep,
        },
    })
}

async fn complete_text(
    client: &dyn LlmClient,
    profile: &SelectorProfile,
    prompt: String,
) -> Result<String, SelectorError> {
    let request = LlmRequest::new(
        &profile.model,
        vec![Message::User(UserMessage::text(prompt))],
    )
    .with_max_tokens(profile.params.max_output_tokens)
    .with_temperature(profile.params.temperature);
    let mut stream = client.stream(&request);
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(classify_llm_error)? {
            LlmEvent::TextDelta { delta, .. } => text.push_str(&delta),
            LlmEvent::Done { outcome } => match outcome {
                LlmDoneOutcome::Success { .. } => break,
                LlmDoneOutcome::Error { error } => return Err(classify_llm_error(error)),
            },
            _ => {}
        }
    }
    Ok(text)
}

fn classify_llm_error(error: LlmError) -> SelectorError {
    match error {
        LlmError::AuthenticationFailed { .. } | LlmError::InvalidApiKey => {
            SelectorError::Auth(error.to_string())
        }
        other => SelectorError::Client(other.to_string()),
    }
}

fn parse_selection(reply: &str) -> Result<RawSelection, String> {
    let trimmed = reply.trim();
    if let Ok(raw) = serde_json::from_str::<RawSelection>(trimmed) {
        return Ok(raw);
    }
    // Tolerate fenced or prefixed output: parse the outermost object.
    let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) else {
        return Err("no JSON object in reply".to_string());
    };
    if start >= end {
        return Err("no JSON object in reply".to_string());
    }
    serde_json::from_str::<RawSelection>(&trimmed[start..=end]).map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------
// Prompt rendering
// ---------------------------------------------------------------------------

fn render_prompt(
    profile: &SelectorProfile,
    manifest: &[RecordMeta],
    turn_text: &str,
    suppressed_ids: &HashSet<String>,
) -> String {
    let rows: Vec<&RecordMeta> = if profile.params.shuffle_manifest {
        shuffled(manifest)
    } else {
        manifest.iter().collect()
    };
    let manifest_text = if rows.is_empty() {
        "(no records)".to_string()
    } else {
        rows.iter()
            .map(|meta| render_manifest_row(meta))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut suppressed: Vec<&str> = suppressed_ids.iter().map(String::as_str).collect();
    suppressed.sort_unstable();
    let suppression_text = if suppressed.is_empty() {
        "(none)".to_string()
    } else {
        suppressed
            .iter()
            .map(|id| format!("- {id}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let turn = truncate_utf8_boundary(&compact_whitespace(turn_text), MAX_PROMPT_TURN_BYTES);
    profile
        .prompt_template
        .replace(MANIFEST_PLACEHOLDER, &manifest_text)
        .replace(SUPPRESSION_PLACEHOLDER, &suppression_text)
        .replace(TURN_TEXT_PLACEHOLDER, &turn)
}

/// One manifest row: id, kind, age, rank, title — description (the fields
/// the prompt bundle promises, in fixture `RecordMeta` shape).
pub(crate) fn render_manifest_row(meta: &RecordMeta) -> String {
    let rank = match meta.rank {
        Some(rank) => format!("rank {rank}"),
        None => "unranked".to_string(),
    };
    let mut row = format!(
        "- {} [{}, {}, {}] {}",
        meta.id,
        meta.kind.as_str(),
        age_phrase(meta.age_days),
        rank,
        compact_whitespace(&meta.title),
    );
    let description = compact_whitespace(&meta.description);
    if !description.is_empty() {
        row.push_str(" — ");
        row.push_str(&description);
    }
    row
}

fn age_phrase(age_days: u64) -> String {
    match age_days {
        0 => "saved today".to_string(),
        1 => "saved 1 day ago".to_string(),
        n => format!("saved {n} days ago"),
    }
}

/// Entropy-seeded Fisher–Yates (§8.3 bias guard). SplitMix64 over an OsRng
/// seed: no `rand` dependency, uniform enough for position shuffling.
fn shuffled(manifest: &[RecordMeta]) -> Vec<&RecordMeta> {
    let mut seed = {
        let mut bytes = [0u8; 8];
        OsRng.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    };
    let mut next = move || {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut rows: Vec<&RecordMeta> = manifest.iter().collect();
    for i in (1..rows.len()).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        rows.swap(i, j);
    }
    rows
}

/// Chunk a Full-tier manifest for the §8.3 scale posture: consecutive rows
/// grouped so each chunk's rendered description text stays under
/// [`FULL_SWEEP_CHUNK_BYTES`] per side-model call.
pub fn chunk_manifest(manifest: &[RecordMeta]) -> Vec<&[RecordMeta]> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut bytes = 0usize;
    for (index, meta) in manifest.iter().enumerate() {
        let row_bytes = render_manifest_row(meta).len() + 1;
        if index > start && bytes + row_bytes > FULL_SWEEP_CHUNK_BYTES {
            chunks.push(&manifest[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += row_bytes;
    }
    if start < manifest.len() {
        chunks.push(&manifest[start..]);
    }
    chunks
}

/// §8.3 hard-ceiling truncation for Full-tier sweeps: keep steward-ranked
/// records (rank ascending — the rank IS the steward's usage-informed
/// ordering, so rank-then-recency is the faithful "oldest-least-used"
/// proxy over `RecordMeta`), then unranked records newest-first, cut at
/// `hard_ceiling`. Returns the kept manifest and the dropped ids so the
/// caller can emit the loud truncation event naming what was dropped.
pub fn truncate_full_manifest(
    mut manifest: Vec<RecordMeta>,
    hard_ceiling: usize,
) -> (Vec<RecordMeta>, Vec<String>) {
    if manifest.len() <= hard_ceiling {
        return (manifest, Vec::new());
    }
    manifest.sort_by(|a, b| match (a.rank, b.rank) {
        (Some(a_rank), Some(b_rank)) => a_rank.cmp(&b_rank),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.age_days.cmp(&b.age_days),
    });
    let dropped = manifest
        .split_off(hard_ceiling)
        .into_iter()
        .map(|meta| meta.id)
        .collect();
    (manifest, dropped)
}

// ---------------------------------------------------------------------------
// Client acquisition (§8.1 invocation seam)
// ---------------------------------------------------------------------------

/// How the stage obtains (and re-obtains) its model client. The real
/// implementation wraps meerkat's factory seam; tests supply a mock.
#[async_trait]
pub trait SelectorHandle: Send + Sync {
    async fn client(&self) -> Result<Arc<dyn LlmClient>, SelectorError>;
    /// Drop any cached client so the next `client()` re-resolves auth.
    fn invalidate(&self);
}

/// Real handle over `AgentFactory::build_llm_client_for_identity`
/// (meerkat 0.7.9 `factory.rs`): realm auth binding + model catalog
/// resolution, the same seam session model hot-swap uses. Clients are
/// cached per `(realm, model)`; [`SelectorHandle::invalidate`] clears the
/// cache so an auth failure re-enters resolution.
pub struct FactorySelectorHandle {
    factory: meerkat::AgentFactory,
    config: meerkat::Config,
    realm: String,
    identity: SessionLlmIdentity,
    cache: Mutex<HashMap<(String, String), Arc<dyn LlmClient>>>,
}

impl FactorySelectorHandle {
    pub fn new(
        store_path: impl Into<PathBuf>,
        config: meerkat::Config,
        realm: impl Into<String>,
        profile: &SelectorProfile,
    ) -> Self {
        Self::for_model(store_path, config, realm, &profile.model, profile.provider)
    }

    /// Same seam, keyed by raw model/provider — the Distiller (§8.4) and any
    /// future off-turn stage obtain their clients through this exact factory
    /// path rather than growing a parallel one (§8.1 dogma rule 7).
    pub fn for_model(
        store_path: impl Into<PathBuf>,
        config: meerkat::Config,
        realm: impl Into<String>,
        model: &str,
        provider: Provider,
    ) -> Self {
        Self {
            factory: meerkat::AgentFactory::new(store_path.into()),
            config,
            realm: realm.into(),
            identity: SessionLlmIdentity {
                model: model.to_string(),
                provider,
                self_hosted_server_id: None,
                provider_params: None,
                // None = the realm's default binding for the provider; the
                // explicit-binding choice is §8.1 open question 3.
                auth_binding: None,
            },
            cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl SelectorHandle for FactorySelectorHandle {
    async fn client(&self) -> Result<Arc<dyn LlmClient>, SelectorError> {
        let key = (self.realm.clone(), self.identity.model.clone());
        if let Some(client) = self
            .cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&key)
        {
            return Ok(client.clone());
        }
        let client = self
            .factory
            .build_llm_client_for_identity(&self.config, &self.identity)
            .await
            .map_err(|err| match err {
                FactoryError::ProviderAuth(_) | FactoryError::ConnectionTarget(_) => {
                    SelectorError::Auth(err.to_string())
                }
                other => SelectorError::Client(other.to_string()),
            })?;
        self.cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(key, client.clone());
        Ok(client)
    }

    fn invalidate(&self) {
        self.cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clear();
    }
}

// ---------------------------------------------------------------------------
// The stage: profile + handle, with auth re-resolve
// ---------------------------------------------------------------------------

/// A configured selector stage. On an auth failure the cached client is
/// invalidated and the call retried once against a freshly resolved client.
pub struct SelectorStage {
    profile: SelectorProfile,
    handle: Arc<dyn SelectorHandle>,
}

impl SelectorStage {
    pub fn new(profile: SelectorProfile, handle: Arc<dyn SelectorHandle>) -> Self {
        Self { profile, handle }
    }

    pub fn profile(&self) -> &SelectorProfile {
        &self.profile
    }

    pub async fn select(
        &self,
        manifest: &[RecordMeta],
        turn_text: &str,
        suppressed_ids: &HashSet<String>,
    ) -> Result<Selection, SelectorError> {
        let client = self.handle.client().await?;
        match select(manifest, turn_text, suppressed_ids, &self.profile, &*client).await {
            Err(SelectorError::Auth(first)) => {
                tracing::warn!(error = %first, "selector auth failure; re-resolving client");
                self.handle.invalidate();
                let client = self.handle.client().await?;
                select(manifest, turn_text, suppressed_ids, &self.profile, &*client).await
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------
// Body fetch for selector-chosen ids
// ---------------------------------------------------------------------------

/// Fetch active record bodies by id across the composed scopes, in the
/// order the ids were selected. Deliberately a standalone trait rather
/// than an `AgentMemoryProvider` method: the v2 provider trait is owned by
/// the recorder/taint cluster and stays untouched; manifest-capable stores
/// opt in here so the coordinator can render selector-chosen bodies.
#[async_trait]
pub trait SelectedRecordFetch: Send + Sync {
    async fn fetch_records(
        &self,
        scopes: &[MemoryScope],
        ids: &[String],
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError>;
}

// ---------------------------------------------------------------------------
// Process-wide install
// ---------------------------------------------------------------------------

/// Everything the coordinator needs when a selector is configured.
pub struct SelectorRuntime {
    pub stage: Arc<SelectorStage>,
    pub fetch: Arc<dyn SelectedRecordFetch>,
}

static INSTALLED: LazyLock<RwLock<Option<Arc<SelectorRuntime>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Install the selector process-wide. Coordinators constructed afterwards
/// pick it up ([`crate::memory::RecallCoordinator`] snapshots at
/// construction); coordinators built for tests inject via
/// `with_selector` and never touch this global.
pub fn install(runtime: Arc<SelectorRuntime>) {
    let mut guard = INSTALLED.write().unwrap_or_else(|err| err.into_inner());
    if guard.is_some() {
        tracing::warn!("agent-memory selector re-installed; replacing the existing stage");
    }
    *guard = Some(runtime);
}

pub fn installed() -> Option<Arc<SelectorRuntime>> {
    INSTALLED
        .read()
        .unwrap_or_else(|err| err.into_inner())
        .clone()
}

/// Parsed operator spec for [`SELECTOR_ENV_VAR`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorSpec {
    Off,
    Default,
    Profile(PathBuf),
}

/// Fail-loud parse of the operator switch: unset or `off` disables the
/// stage; `default` uses the embedded profile; `profile:<path>` loads an
/// external calibration profile.
pub fn spec_from_env() -> Result<SelectorSpec, SelectorError> {
    match std::env::var(SELECTOR_ENV_VAR) {
        Err(std::env::VarError::NotPresent) => Ok(SelectorSpec::Off),
        Err(err) => Err(SelectorError::Profile(format!(
            "{SELECTOR_ENV_VAR} is not valid unicode: {err}"
        ))),
        Ok(value) => parse_spec(&value),
    }
}

fn parse_spec(value: &str) -> Result<SelectorSpec, SelectorError> {
    let value = value.trim();
    match value {
        "" | "off" => Ok(SelectorSpec::Off),
        "default" => Ok(SelectorSpec::Default),
        other => match other.strip_prefix("profile:") {
            Some(path) if !path.trim().is_empty() => {
                Ok(SelectorSpec::Profile(PathBuf::from(path.trim())))
            }
            _ => Err(SelectorError::Profile(format!(
                "invalid {SELECTOR_ENV_VAR} value '{other}' \
                 (expected off | default | profile:<path>)"
            ))),
        },
    }
}

/// Resolve a spec to a loaded profile (`Off` → `None`).
pub fn profile_for_spec(spec: &SelectorSpec) -> Result<Option<SelectorProfile>, SelectorError> {
    match spec {
        SelectorSpec::Off => Ok(None),
        SelectorSpec::Default => Ok(Some(SelectorProfile::embedded_default())),
        SelectorSpec::Profile(path) => SelectorProfile::load(path).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::records::MemoryKind;
    use futures::stream;
    use std::sync::Mutex as StdMutex;

    fn meta(id: &str, title: &str, description: &str) -> RecordMeta {
        RecordMeta {
            id: id.to_string(),
            kind: MemoryKind::Gotcha,
            title: title.to_string(),
            description: description.to_string(),
            age_days: 3,
            rank: Some(1),
        }
    }

    /// Scripted mock: returns canned replies in order and captures every
    /// prompt it was sent.
    struct ScriptedLlm {
        replies: StdMutex<Vec<String>>,
        prompts: StdMutex<Vec<String>>,
        provider: Provider,
    }

    impl ScriptedLlm {
        fn new(replies: Vec<&str>) -> Self {
            Self {
                replies: StdMutex::new(replies.into_iter().map(str::to_string).collect()),
                prompts: StdMutex::new(Vec::new()),
                provider: Provider::Anthropic,
            }
        }

        fn prompts(&self) -> Vec<String> {
            self.prompts
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .clone()
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlm {
        fn stream<'a>(&'a self, request: &'a LlmRequest) -> meerkat_client::types::LlmStream<'a> {
            let prompt = request
                .messages
                .iter()
                .map(|message| match message {
                    Message::User(user) => user.text_content(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.prompts
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(prompt);
            let reply = {
                let mut replies = self.replies.lock().unwrap_or_else(|err| err.into_inner());
                if replies.is_empty() {
                    String::new()
                } else {
                    replies.remove(0)
                }
            };
            Box::pin(stream::iter(vec![
                Ok(LlmEvent::TextDelta {
                    delta: reply,
                    meta: None,
                }),
                Ok(LlmEvent::Done {
                    outcome: LlmDoneOutcome::Success {
                        stop_reason: meerkat_core::StopReason::EndTurn,
                    },
                }),
            ]))
        }

        fn provider(&self) -> Provider {
            self.provider
        }

        async fn health_check(&self) -> Result<(), LlmError> {
            Ok(())
        }
    }

    fn manifest() -> Vec<RecordMeta> {
        vec![
            meta("mem-1", "Cargo wrapper", "When running cargo commands"),
            meta("mem-2", "Deploy freeze", "When deploying on Fridays"),
            meta(
                "mem-3",
                "Passport location",
                "When travel documents come up",
            ),
        ]
    }

    #[tokio::test]
    async fn select_parses_and_orders_selection() -> Result<(), Box<dyn std::error::Error>> {
        let client = ScriptedLlm::new(vec![
            r#"{"selected_ids": ["mem-2", "mem-1"], "coverage": "sufficient"}"#,
        ]);
        let profile = SelectorProfile::embedded_default();
        let selection = select(
            &manifest(),
            "deploying the gateway",
            &HashSet::new(),
            &profile,
            &client,
        )
        .await?;
        assert_eq!(selection.selected_ids, vec!["mem-2", "mem-1"]);
        assert_eq!(selection.coverage, Coverage::Sufficient);
        Ok(())
    }

    #[tokio::test]
    async fn select_drops_unknown_and_suppressed_ids() -> Result<(), Box<dyn std::error::Error>> {
        let client = ScriptedLlm::new(vec![
            r#"{"selected_ids": ["mem-9", "mem-1", "mem-2", "mem-1"], "coverage": "need_deeper_sweep"}"#,
        ]);
        let profile = SelectorProfile::embedded_default();
        let suppressed: HashSet<String> = ["mem-2".to_string()].into();
        let selection = select(&manifest(), "cargo build", &suppressed, &profile, &client).await?;
        assert_eq!(selection.selected_ids, vec!["mem-1"]);
        assert_eq!(selection.coverage, Coverage::NeedDeeperSweep);
        Ok(())
    }

    #[tokio::test]
    async fn select_repairs_malformed_json_once() -> Result<(), Box<dyn std::error::Error>> {
        let client = ScriptedLlm::new(vec![
            "Sure! Here is my selection, hope it helps",
            r#"{"selected_ids": ["mem-3"], "coverage": "sufficient"}"#,
        ]);
        let profile = SelectorProfile::embedded_default();
        let selection = select(
            &manifest(),
            "where is my passport",
            &HashSet::new(),
            &profile,
            &client,
        )
        .await?;
        assert_eq!(selection.selected_ids, vec!["mem-3"]);
        let prompts = client.prompts();
        assert_eq!(prompts.len(), 2, "exactly one repair round-trip");
        assert!(prompts[1].contains("ONLY the corrected JSON object"));
        Ok(())
    }

    #[tokio::test]
    async fn select_errors_after_failed_repair() {
        let client = ScriptedLlm::new(vec!["not json", "still not json"]);
        let profile = SelectorProfile::embedded_default();
        let result = select(&manifest(), "turn", &HashSet::new(), &profile, &client).await;
        assert!(matches!(result, Err(SelectorError::Parse(_))), "{result:?}");
        assert_eq!(client.prompts().len(), 2);
    }

    #[tokio::test]
    async fn select_tolerates_fenced_json() -> Result<(), Box<dyn std::error::Error>> {
        let client = ScriptedLlm::new(vec![
            "```json\n{\"selected_ids\": [\"mem-1\"], \"coverage\": \"sufficient\"}\n```",
        ]);
        let profile = SelectorProfile::embedded_default();
        let selection = select(
            &manifest(),
            "cargo check",
            &HashSet::new(),
            &profile,
            &client,
        )
        .await?;
        assert_eq!(selection.selected_ids, vec!["mem-1"]);
        assert_eq!(client.prompts().len(), 1, "fenced JSON needs no repair");
        Ok(())
    }

    #[tokio::test]
    async fn prompt_renders_manifest_suppression_and_turn() -> Result<(), Box<dyn std::error::Error>>
    {
        let client = ScriptedLlm::new(vec![r#"{"selected_ids": [], "coverage": "sufficient"}"#]);
        let profile = SelectorProfile::embedded_default();
        let suppressed: HashSet<String> = ["mem-3".to_string()].into();
        select(
            &manifest(),
            "the incoming turn text",
            &suppressed,
            &profile,
            &client,
        )
        .await?;
        let prompt = &client.prompts()[0];
        assert!(prompt.contains("- mem-1 [gotcha, saved 3 days ago, rank 1] Cargo wrapper"));
        assert!(prompt.contains("— When running cargo commands"), "{prompt}");
        assert!(
            prompt.contains("- mem-3\n") || prompt.ends_with("- mem-3"),
            "{prompt}"
        );
        assert!(prompt.contains("the incoming turn text"));
        assert!(!prompt.contains("{{manifest}}"));
        assert!(!prompt.contains("{{turn_text}}"));
        assert!(!prompt.contains("{{suppression_list}}"));
        Ok(())
    }

    #[tokio::test]
    async fn shuffle_changes_manifest_order_across_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        // 12 records have 12! orderings; 24 shuffles landing identical is
        // vanishingly unlikely, so a stuck shuffle fails deterministically.
        let manifest: Vec<RecordMeta> = (0..12)
            .map(|i| meta(&format!("mem-{i}"), &format!("Title {i}"), ""))
            .collect();
        let replies = vec![r#"{"selected_ids": [], "coverage": "sufficient"}"#; 24];
        let client = ScriptedLlm::new(replies);
        let profile = SelectorProfile::embedded_default();
        for _ in 0..24 {
            select(&manifest, "turn", &HashSet::new(), &profile, &client).await?;
        }
        let prompts = client.prompts();
        let orders: HashSet<String> = prompts
            .iter()
            .map(|prompt| {
                prompt
                    .lines()
                    .filter(|line| line.starts_with("- mem-"))
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect();
        assert!(
            orders.len() > 1,
            "manifest order must vary across calls (§8.3 bias guard)"
        );
        Ok(())
    }

    #[test]
    fn embedded_prompt_matches_calibration_bundle() -> Result<(), Box<dyn std::error::Error>> {
        // The crate-local embed and the memory-evals calibration artifact
        // must stay byte-identical; skip when the evals tree is absent
        // (published crate builds).
        let bundle =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../memory-evals/prompts/selector-v0.md");
        if !bundle.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(bundle)?;
        assert_eq!(
            text, EMBEDDED_PROMPT_V0,
            "memory-evals/prompts/selector-v0.md and \
             src/memory/selector_prompt_v0.md have drifted"
        );
        Ok(())
    }

    #[test]
    fn embedded_default_profile_validates_and_names_a_catalog_model() {
        let profile = SelectorProfile::embedded_default();
        profile.validate().expect("embedded profile must validate");
        assert_eq!(
            meerkat_models::infer_provider(&profile.model),
            Some(profile.provider),
            "embedded default model must resolve in the catalog"
        );
    }

    #[test]
    fn external_profile_loads_from_evals_layout() -> Result<(), Box<dyn std::error::Error>> {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../memory-evals/profiles/selector-v0.toml");
        if !path.is_file() {
            return Ok(());
        }
        let profile = SelectorProfile::load(&path)?;
        assert_eq!(profile.stage, "selector");
        assert_eq!(profile.model, SelectorProfile::embedded_default().model);
        assert_eq!(profile.prompt_template, EMBEDDED_PROMPT_V0);
        assert_eq!(profile.params.working_set_k, 200);
        Ok(())
    }

    #[test]
    fn spec_parse_is_fail_loud() {
        assert_eq!(parse_spec("off").unwrap(), SelectorSpec::Off);
        assert_eq!(parse_spec("").unwrap(), SelectorSpec::Off);
        assert_eq!(parse_spec("default").unwrap(), SelectorSpec::Default);
        assert_eq!(
            parse_spec("profile:/tmp/p.toml").unwrap(),
            SelectorSpec::Profile(PathBuf::from("/tmp/p.toml"))
        );
        assert!(parse_spec("lexical").is_err());
        assert!(parse_spec("profile:").is_err());
    }

    #[test]
    fn hard_ceiling_truncation_drops_oldest_unranked_and_names_them() {
        let record = |id: &str, rank: Option<u32>, age_days: u64| RecordMeta {
            id: id.to_string(),
            kind: MemoryKind::Fact,
            title: "t".to_string(),
            description: String::new(),
            age_days,
            rank,
        };
        // Below the ceiling: untouched, order preserved, nothing dropped.
        let manifest = vec![record("a", None, 90), record("b", Some(2), 1)];
        let (kept, dropped) = truncate_full_manifest(manifest.clone(), 2);
        assert_eq!(
            kept.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert!(dropped.is_empty());

        // Above the ceiling: ranked survive (rank ascending), then unranked
        // newest-first; the oldest unranked records are the ones dropped —
        // and they are named.
        let manifest = vec![
            record("old-unranked", None, 90),
            record("rank-2", Some(2), 50),
            record("new-unranked", None, 0),
            record("rank-1", Some(1), 70),
            record("mid-unranked", None, 30),
        ];
        let (kept, dropped) = truncate_full_manifest(manifest, 3);
        assert_eq!(
            kept.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["rank-1", "rank-2", "new-unranked"]
        );
        assert_eq!(dropped, vec!["mid-unranked", "old-unranked"]);
    }

    #[test]
    fn chunking_respects_byte_ceiling_and_covers_everything() {
        let big_description = "d".repeat(390);
        let manifest: Vec<RecordMeta> = (0..600)
            .map(|i| meta(&format!("mem-{i}"), "Title", &big_description))
            .collect();
        let chunks = chunk_manifest(&manifest);
        assert!(chunks.len() > 1, "600 fat rows must not fit one chunk");
        let total: usize = chunks.iter().map(|chunk| chunk.len()).sum();
        assert_eq!(total, manifest.len(), "chunking must not drop rows");
        for chunk in &chunks {
            let bytes: usize = chunk
                .iter()
                .map(|meta| render_manifest_row(meta).len() + 1)
                .sum();
            assert!(
                bytes <= FULL_SWEEP_CHUNK_BYTES,
                "chunk exceeds the §8.3 per-call ceiling: {bytes}"
            );
        }
    }
}
