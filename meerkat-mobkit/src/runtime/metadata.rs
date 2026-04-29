//! Mobkit-side sidecar table for mob- and run-level labels.
//!
//! Member-level labels are owned by `meerkat-mob` (they flow through
//! `SpawnMemberSpec.with_labels()` and out via `MobMemberListEntry.labels`).
//! Mob-level and run-level labels — for associating an external context like
//! `repo`, `branch`, `customer`, `deployment`, or `environment` with a mob or
//! a flow run — have nowhere to live in the upstream model. This module owns
//! that side table.
//!
//! For v1 the table is in-memory only. Persistence behind `MobStorage` is a
//! future enhancement; restarts wipe the labels. The table is keyed by
//! [`MetadataScope`] so the same surface can serve mobs and runs uniformly.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

/// Scope of a label set.
///
/// Mob scope holds labels keyed by `mob_id`; run scope holds labels keyed by
/// `(mob_id, run_id)`. The mob id is part of the run scope so two mobs with
/// overlapping run identifiers stay isolated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MetadataScope {
    Mob(String),
    Run(String, String),
}

impl MetadataScope {
    /// Return the mob id this scope belongs to.
    pub fn mob_id(&self) -> &str {
        match self {
            Self::Mob(mob) => mob,
            Self::Run(mob, _) => mob,
        }
    }

    /// Return the run id, if this scope is run-scoped.
    pub fn run_id(&self) -> Option<&str> {
        match self {
            Self::Mob(_) => None,
            Self::Run(_, run) => Some(run),
        }
    }
}

/// In-memory label table keyed by [`MetadataScope`].
///
/// Operations replace label sets wholesale (no merge). Callers wanting
/// merge semantics should read first, mutate the map, then write it back.
#[derive(Debug, Clone, Default)]
pub struct RuntimeMetadataTable {
    inner: Arc<RwLock<BTreeMap<MetadataScope, BTreeMap<String, String>>>>,
}

impl RuntimeMetadataTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the label set for `scope`. An empty `labels` map clears
    /// the entry.
    pub async fn set_labels(&self, scope: MetadataScope, labels: BTreeMap<String, String>) {
        let mut guard = self.inner.write().await;
        if labels.is_empty() {
            guard.remove(&scope);
        } else {
            guard.insert(scope, labels);
        }
    }

    /// Return the label set for `scope`, or an empty map if none is set.
    pub async fn get_labels(&self, scope: &MetadataScope) -> BTreeMap<String, String> {
        let guard = self.inner.read().await;
        guard.get(scope).cloned().unwrap_or_default()
    }

    /// Remove the label set for `scope`. Returns the previous value if any.
    pub async fn delete_labels(&self, scope: &MetadataScope) -> Option<BTreeMap<String, String>> {
        let mut guard = self.inner.write().await;
        guard.remove(scope)
    }

    /// Return all label sets associated with a mob — both the mob-scoped
    /// entry (if any) and every run-scoped entry whose mob id matches.
    pub async fn list_labels_for_mob(
        &self,
        mob_id: &str,
    ) -> Vec<(MetadataScope, BTreeMap<String, String>)> {
        let guard = self.inner.read().await;
        guard
            .iter()
            .filter(|(scope, _)| scope.mob_id() == mob_id)
            .map(|(scope, labels)| (scope.clone(), labels.clone()))
            .collect()
    }
}

/// Parse a JSON `labels` field as a string→string map.
///
/// Accepts a missing field, `null`, or an empty object — all yield an empty
/// map. Anything else must deserialize cleanly or returns a human-readable
/// error string suitable for a JSON-RPC `Invalid params` reply.
pub fn parse_labels_param(value: Option<&Value>) -> Result<BTreeMap<String, String>, String> {
    match value {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(v) => serde_json::from_value::<BTreeMap<String, String>>(v.clone())
            .map_err(|err| format!("labels must be a map of string to string: {err}")),
    }
}

/// Render a label map as a JSON object suitable for the wire format.
pub fn labels_to_json_value(labels: &BTreeMap<String, String>) -> Value {
    let mut map = serde_json::Map::with_capacity(labels.len());
    for (k, v) in labels {
        map.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(map)
}

/// Outcome of dispatching a label RPC against a [`RuntimeMetadataTable`].
///
/// Both transports (the unified-runtime JSON-RPC and the HTTP-console JSON-RPC)
/// project this into their own response envelope.
pub enum LabelRpcResult {
    /// `set` / `delete`: returns `{"accepted": true}`.
    Accepted,
    /// `get`: returns `{"labels": {...}}`.
    Labels(BTreeMap<String, String>),
    /// Validation error — `Invalid params: <message>`.
    InvalidParams(String),
}

/// Replace the label set for `scope`, parsing `labels` from RPC params.
pub async fn dispatch_labels_set(
    table: &RuntimeMetadataTable,
    scope: MetadataScope,
    params: &Value,
) -> LabelRpcResult {
    match parse_labels_param(params.get("labels")) {
        Ok(labels) => {
            table.set_labels(scope, labels).await;
            LabelRpcResult::Accepted
        }
        Err(message) => LabelRpcResult::InvalidParams(message),
    }
}

/// Read the label set for `scope`.
pub async fn dispatch_labels_get(
    table: &RuntimeMetadataTable,
    scope: MetadataScope,
) -> LabelRpcResult {
    LabelRpcResult::Labels(table.get_labels(&scope).await)
}

/// Remove the label set for `scope`.
pub async fn dispatch_labels_delete(
    table: &RuntimeMetadataTable,
    scope: MetadataScope,
) -> LabelRpcResult {
    let _ = table.delete_labels(&scope).await;
    LabelRpcResult::Accepted
}

/// Pull a non-empty `run_id` string from RPC params.
pub fn parse_run_id_param(params: &Value) -> Result<&str, String> {
    match params.get("run_id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err("run_id required".to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[tokio::test]
    async fn set_and_get_mob_labels() {
        let table = RuntimeMetadataTable::new();
        let scope = MetadataScope::Mob("mob-a".to_string());
        table
            .set_labels(scope.clone(), labels(&[("repo", "agents"), ("env", "dev")]))
            .await;
        let got = table.get_labels(&scope).await;
        assert_eq!(got.get("repo").map(String::as_str), Some("agents"));
        assert_eq!(got.get("env").map(String::as_str), Some("dev"));
    }

    #[tokio::test]
    async fn set_replaces_rather_than_merges() {
        let table = RuntimeMetadataTable::new();
        let scope = MetadataScope::Mob("mob-a".to_string());
        table
            .set_labels(scope.clone(), labels(&[("a", "1"), ("b", "2")]))
            .await;
        table.set_labels(scope.clone(), labels(&[("a", "9")])).await;
        let got = table.get_labels(&scope).await;
        assert_eq!(got.len(), 1);
        assert_eq!(got.get("a").map(String::as_str), Some("9"));
        assert!(!got.contains_key("b"));
    }

    #[tokio::test]
    async fn delete_clears_entry() {
        let table = RuntimeMetadataTable::new();
        let scope = MetadataScope::Run("mob-a".to_string(), "run-1".to_string());
        table.set_labels(scope.clone(), labels(&[("k", "v")])).await;
        let prev = table.delete_labels(&scope).await;
        assert_eq!(prev.unwrap().get("k").map(String::as_str), Some("v"));
        let after = table.get_labels(&scope).await;
        assert!(after.is_empty());
    }

    #[tokio::test]
    async fn empty_set_clears_entry() {
        let table = RuntimeMetadataTable::new();
        let scope = MetadataScope::Mob("mob-a".to_string());
        table.set_labels(scope.clone(), labels(&[("k", "v")])).await;
        table.set_labels(scope.clone(), BTreeMap::new()).await;
        assert!(table.get_labels(&scope).await.is_empty());
    }

    #[tokio::test]
    async fn list_returns_mob_and_run_entries() {
        let table = RuntimeMetadataTable::new();
        let mob_scope = MetadataScope::Mob("mob-a".to_string());
        let run_scope = MetadataScope::Run("mob-a".to_string(), "run-1".to_string());
        let other_run = MetadataScope::Run("mob-b".to_string(), "run-1".to_string());
        table
            .set_labels(mob_scope.clone(), labels(&[("env", "dev")]))
            .await;
        table
            .set_labels(run_scope.clone(), labels(&[("trace", "abc")]))
            .await;
        table
            .set_labels(other_run, labels(&[("trace", "xyz")]))
            .await;

        let entries = table.list_labels_for_mob("mob-a").await;
        assert_eq!(entries.len(), 2);
        let scopes: Vec<&MetadataScope> = entries.iter().map(|(s, _)| s).collect();
        assert!(scopes.contains(&&mob_scope));
        assert!(scopes.contains(&&run_scope));
    }

    #[tokio::test]
    async fn run_scope_distinguishes_mobs() {
        let table = RuntimeMetadataTable::new();
        let scope_a = MetadataScope::Run("mob-a".to_string(), "run-1".to_string());
        let scope_b = MetadataScope::Run("mob-b".to_string(), "run-1".to_string());
        table
            .set_labels(scope_a.clone(), labels(&[("k", "a")]))
            .await;
        table
            .set_labels(scope_b.clone(), labels(&[("k", "b")]))
            .await;
        assert_eq!(
            table
                .get_labels(&scope_a)
                .await
                .get("k")
                .map(String::as_str),
            Some("a")
        );
        assert_eq!(
            table
                .get_labels(&scope_b)
                .await
                .get("k")
                .map(String::as_str),
            Some("b")
        );
    }
}
