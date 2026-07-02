//! selector-eval — calibration-harness seam for the memory Selector
//! (docs/design/agent-memory-architecture.md §8.3/§11).
//!
//! Reads one fixture-shaped JSON object on stdin:
//!
//! ```json
//! {"manifest": [RecordMeta...], "turn_text": "...",
//!  "suppressed_ids": ["..."], "profile_path": "optional/path.toml"}
//! ```
//!
//! and writes `{"selected_ids": [...], "coverage": "..."}` on stdout.
//! `--mock` swaps the live model for a deterministic scripted client whose
//! selection is computed from the input (title-word overlap, sorted by id),
//! so the harness's live plumbing — prompt rendering, manifest shuffle,
//! strict JSON parsing, validation — runs end-to-end without credentials.
//!
//! Exit codes: 0 ok, 1 selector error, 2 usage/config error, 3 live
//! requested but no auth is resolvable (the harness treats 3 as
//! SKIP-with-notice, not failure).

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use serde::Deserialize;

use meerkat_client::types::LlmStream;
use meerkat_client::{LlmClient, LlmDoneOutcome, LlmError, LlmEvent, LlmRequest};
use meerkat_core::Provider;
use meerkat_mobkit::memory::records::{MemoryKind, RecordMeta};
use meerkat_mobkit::memory::selector::{
    Coverage, FactorySelectorHandle, SelectorError, SelectorHandle, SelectorProfile, select,
};

const EXIT_SELECTOR_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
const EXIT_NO_AUTH: i32 = 3;

/// Manifest entry in the calibration-fixture format (memory-evals/README:
/// capitalized kinds), mapped onto the crate's `RecordMeta`.
#[derive(Deserialize)]
struct FixtureRecordMeta {
    id: String,
    kind: String,
    title: String,
    #[serde(default)]
    description: String,
    age_days: u64,
    #[serde(default)]
    rank: Option<u32>,
}

impl FixtureRecordMeta {
    fn into_record_meta(self) -> Result<RecordMeta, String> {
        let kind = match self.kind.as_str() {
            "Preference" => MemoryKind::Preference,
            "Fact" => MemoryKind::Fact,
            "Gotcha" => MemoryKind::Gotcha,
            "Procedure" => MemoryKind::Procedure,
            "Relationship" => MemoryKind::Relationship,
            "OpenLoop" => MemoryKind::OpenLoop,
            "Reference" => MemoryKind::Reference,
            other => match MemoryKind::parse(other) {
                Some(kind) => kind,
                None => return Err(format!("unknown record kind '{other}'")),
            },
        };
        Ok(RecordMeta {
            id: self.id,
            kind,
            title: self.title,
            description: self.description,
            age_days: self.age_days,
            rank: self.rank,
        })
    }
}

#[derive(Deserialize)]
struct EvalInput {
    manifest: Vec<FixtureRecordMeta>,
    turn_text: String,
    #[serde(default)]
    suppressed_ids: Vec<String>,
    #[serde(default)]
    profile_path: Option<PathBuf>,
}

/// Deterministic stand-in for the live model: replies with the ids whose
/// title words overlap the turn text (the same crude rule as the harness's
/// legacy in-python mock), sorted by id so the output is shuffle-stable.
/// The reply goes through the real prompt/parse path in `select`.
struct MockSelectorLlm {
    reply: String,
}

impl MockSelectorLlm {
    fn for_input(input: &EvalInput, manifest: &[RecordMeta]) -> Self {
        let suppressed: HashSet<&str> = input.suppressed_ids.iter().map(String::as_str).collect();
        let turn_tokens = tokens(&input.turn_text);
        let mut ids: Vec<&str> = manifest
            .iter()
            .filter(|meta| {
                !suppressed.contains(meta.id.as_str())
                    && tokens(&meta.title).iter().any(|t| turn_tokens.contains(t))
            })
            .map(|meta| meta.id.as_str())
            .collect();
        ids.sort_unstable();
        let reply = serde_json::json!({
            "selected_ids": ids,
            "coverage": "sufficient",
        });
        Self {
            reply: reply.to_string(),
        }
    }
}

fn tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 4)
        .map(str::to_string)
        .collect()
}

#[async_trait]
impl LlmClient for MockSelectorLlm {
    fn stream<'a>(&'a self, _request: &'a LlmRequest) -> LlmStream<'a> {
        Box::pin(stream::iter(vec![
            Ok(LlmEvent::TextDelta {
                delta: self.reply.clone(),
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
        Provider::Other
    }

    async fn health_check(&self) -> Result<(), LlmError> {
        Ok(())
    }
}

fn fail(code: i32, message: &str) -> ! {
    eprintln!("selector-eval: {message}");
    std::process::exit(code);
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut mock = false;
    for arg in &args {
        match arg.as_str() {
            "--mock" => mock = true,
            "--help" | "-h" => {
                eprintln!(
                    "usage: selector-eval [--mock] < fixture.json\n\
                     stdin:  {{manifest, turn_text, suppressed_ids?, profile_path?}}\n\
                     stdout: {{selected_ids, coverage}}\n\
                     exit:   0 ok, 1 error, 2 usage, 3 live-without-auth (SKIP)"
                );
                std::process::exit(0);
            }
            other => fail(EXIT_USAGE, &format!("unknown argument '{other}'")),
        }
    }

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() || raw.trim().is_empty() {
        fail(EXIT_USAGE, "expected a fixture JSON object on stdin");
    }
    let mut input: EvalInput = match serde_json::from_str(&raw) {
        Ok(input) => input,
        Err(err) => fail(EXIT_USAGE, &format!("invalid fixture JSON: {err}")),
    };
    let manifest: Vec<RecordMeta> = match std::mem::take(&mut input.manifest)
        .into_iter()
        .map(FixtureRecordMeta::into_record_meta)
        .collect::<Result<_, _>>()
    {
        Ok(manifest) => manifest,
        Err(err) => fail(EXIT_USAGE, &format!("invalid fixture manifest: {err}")),
    };

    let profile = match input.profile_path.as_deref() {
        Some(path) => match SelectorProfile::load(path) {
            Ok(profile) => profile,
            Err(err) => fail(EXIT_USAGE, &err.to_string()),
        },
        None => SelectorProfile::embedded_default(),
    };

    let client: Arc<dyn LlmClient> = if mock {
        Arc::new(MockSelectorLlm::for_input(&input, &manifest))
    } else {
        // Auth resolves through meerkat's factory seam against the process
        // environment (realm default bindings / provider env keys). A
        // resolution failure is the SKIP signal, not an error.
        let state_dir = match tempfile::tempdir() {
            Ok(dir) => dir.keep(),
            Err(err) => fail(
                EXIT_SELECTOR_ERROR,
                &format!("cannot create scratch state dir: {err}"),
            ),
        };
        let handle =
            FactorySelectorHandle::new(state_dir, meerkat::Config::default(), "default", &profile);
        match handle.client().await {
            Ok(client) => client,
            Err(SelectorError::Auth(message)) => fail(
                EXIT_NO_AUTH,
                &format!("no resolvable auth for live selector run: {message}"),
            ),
            Err(err) => fail(EXIT_SELECTOR_ERROR, &err.to_string()),
        }
    };

    let suppressed: HashSet<String> = input.suppressed_ids.iter().cloned().collect();
    let selection = match select(&manifest, &input.turn_text, &suppressed, &profile, &*client).await
    {
        Ok(selection) => selection,
        Err(SelectorError::Auth(message)) if !mock => fail(
            EXIT_NO_AUTH,
            &format!("auth rejected during live selector run: {message}"),
        ),
        Err(err) => fail(EXIT_SELECTOR_ERROR, &err.to_string()),
    };

    let coverage = match selection.coverage {
        Coverage::Sufficient => "sufficient",
        Coverage::NeedDeeperSweep => "need_deeper_sweep",
    };
    println!(
        "{}",
        serde_json::json!({
            "selected_ids": selection.selected_ids,
            "coverage": coverage,
        })
    );
}
