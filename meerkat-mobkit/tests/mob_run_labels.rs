#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]
//! End-to-end test for the mob/run label sidecar.
//!
//! Drives `RuntimeMetadataTable` (the in-memory store that backs
//! `UnifiedRuntime::set_mob_labels` / `set_run_labels`) directly. Bootstrapping
//! a full `UnifiedRuntime` requires LLM clients, mob definitions, and storage
//! adapters — none of which the metadata side car depends on, so we exercise
//! the table at its boundary.

use std::collections::BTreeMap;

use meerkat_mobkit::{MetadataScope, RuntimeMetadataTable};

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[tokio::test]
async fn mob_and_run_labels_set_get_delete_round_trip() {
    let table = RuntimeMetadataTable::new();
    let mob = "mob-fixture".to_string();
    let mob_scope = MetadataScope::Mob(mob.clone());
    let run_a = MetadataScope::Run(mob.clone(), "run-a".to_string());
    let run_b = MetadataScope::Run(mob.clone(), "run-b".to_string());

    table
        .set_labels(
            mob_scope.clone(),
            labels(&[("repo", "agents"), ("env", "dev")]),
        )
        .await;
    table
        .set_labels(run_a.clone(), labels(&[("trace_id", "alpha")]))
        .await;
    table
        .set_labels(run_b.clone(), labels(&[("trace_id", "beta")]))
        .await;

    // Read everything back.
    let got_mob = table.get_labels(&mob_scope).await;
    assert_eq!(got_mob.get("repo").map(String::as_str), Some("agents"));
    assert_eq!(got_mob.get("env").map(String::as_str), Some("dev"));
    let got_a = table.get_labels(&run_a).await;
    assert_eq!(got_a.get("trace_id").map(String::as_str), Some("alpha"));
    let got_b = table.get_labels(&run_b).await;
    assert_eq!(got_b.get("trace_id").map(String::as_str), Some("beta"));

    // Listing returns mob + both runs.
    let listed = table.list_labels_for_mob(&mob).await;
    assert_eq!(listed.len(), 3);
    let scopes: Vec<&MetadataScope> = listed.iter().map(|(s, _)| s).collect();
    assert!(scopes.contains(&&mob_scope));
    assert!(scopes.contains(&&run_a));
    assert!(scopes.contains(&&run_b));

    // Replacement is wholesale (not a merge).
    table
        .set_labels(mob_scope.clone(), labels(&[("repo", "platform")]))
        .await;
    let after_replace = table.get_labels(&mob_scope).await;
    assert_eq!(after_replace.len(), 1);
    assert_eq!(
        after_replace.get("repo").map(String::as_str),
        Some("platform")
    );
    assert!(!after_replace.contains_key("env"));

    // Delete one run; the others remain.
    let prev = table.delete_labels(&run_a).await;
    assert_eq!(
        prev.unwrap().get("trace_id").map(String::as_str),
        Some("alpha")
    );
    assert!(table.get_labels(&run_a).await.is_empty());
    assert_eq!(
        table
            .get_labels(&run_b)
            .await
            .get("trace_id")
            .map(String::as_str),
        Some("beta"),
    );
    assert!(!table.get_labels(&mob_scope).await.is_empty());
}

#[tokio::test]
async fn run_scope_isolated_per_mob() {
    let table = RuntimeMetadataTable::new();
    let scope_a = MetadataScope::Run("mob-a".to_string(), "run-1".to_string());
    let scope_b = MetadataScope::Run("mob-b".to_string(), "run-1".to_string());
    table
        .set_labels(scope_a.clone(), labels(&[("k", "value-a")]))
        .await;
    table
        .set_labels(scope_b.clone(), labels(&[("k", "value-b")]))
        .await;
    assert_eq!(
        table
            .get_labels(&scope_a)
            .await
            .get("k")
            .map(String::as_str),
        Some("value-a")
    );
    assert_eq!(
        table
            .get_labels(&scope_b)
            .await
            .get("k")
            .map(String::as_str),
        Some("value-b")
    );

    let listed_a = table.list_labels_for_mob("mob-a").await;
    assert_eq!(listed_a.len(), 1);
    let listed_b = table.list_labels_for_mob("mob-b").await;
    assert_eq!(listed_b.len(), 1);
}

#[tokio::test]
async fn empty_label_set_clears_entry() {
    let table = RuntimeMetadataTable::new();
    let scope = MetadataScope::Mob("mob-x".to_string());
    table.set_labels(scope.clone(), labels(&[("a", "1")])).await;
    table.set_labels(scope.clone(), BTreeMap::new()).await;
    assert!(table.get_labels(&scope).await.is_empty());
}

#[tokio::test]
async fn delete_missing_returns_none() {
    let table = RuntimeMetadataTable::new();
    let scope = MetadataScope::Mob("never-set".to_string());
    assert!(table.delete_labels(&scope).await.is_none());
}
