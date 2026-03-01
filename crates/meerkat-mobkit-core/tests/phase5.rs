use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use meerkat_mobkit_core::{
    BigQuerySessionStoreAdapter, JsonFileSessionStore, JsonFileSessionStoreError,
    JsonStoreLockRecord, SessionPersistenceRow,
};
use serde_json::json;
use tempfile::tempdir;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn build_fake_bq_script(
    script_path: &std::path::Path,
    insert_log: &std::path::Path,
    insert_args_log: &std::path::Path,
    query_result: &std::path::Path,
    query_args_log: &std::path::Path,
) {
    let script = format!(
        r#"#!/bin/sh
set -eu
cmd="$1"
shift
case "$cmd" in
  insert)
    table="$1"
    shift
    printf 'insert %s %s\n' "$table" "$*" > "{insert_args_log}"
    cat >> "{insert_log}"
    ;;
  query)
    printf 'query %s\n' "$*" > "{query_args_log}"
    cat "{query_result}"
    ;;
  *)
    echo "unsupported command: $cmd" >&2
    exit 2
    ;;
esac
"#,
        insert_args_log = insert_args_log.display(),
        insert_log = insert_log.display(),
        query_args_log = query_args_log.display(),
        query_result = query_result.display()
    );
    fs::write(script_path, script).expect("fake bq script should be created");
    let mut perms = fs::metadata(script_path)
        .expect("fake bq script metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).expect("fake bq script should be executable");
}

#[test]
fn phase5_json_store_recovers_stale_lock_and_persists_rows() {
    let temp = tempdir().expect("tempdir");
    let sessions_path = temp.path().join("sessions.json");
    let store = JsonFileSessionStore::new(&sessions_path)
        .with_stale_lock_threshold(Duration::from_millis(10));

    let stale_record = JsonStoreLockRecord {
        owner_pid: 999_999,
        created_at_ms: now_ms().saturating_sub(10_000),
    };
    fs::write(
        store.lock_path(),
        serde_json::to_vec(&stale_record).expect("serialize stale lock record"),
    )
    .expect("write stale lock file");

    let writes = vec![SessionPersistenceRow {
        session_id: "s1".to_string(),
        updated_at_ms: 100,
        deleted: false,
        payload: json!({"step":"create"}),
    }];
    store
        .append_rows(&writes)
        .expect("stale lock should be recovered");

    let persisted = store.read_rows().expect("rows should be persisted");
    assert_eq!(persisted, writes);
    assert!(
        !store.lock_path().exists(),
        "lock file should be cleaned up after successful write"
    );
}

#[test]
fn phase5_json_store_blocks_on_fresh_lock() {
    let temp = tempdir().expect("tempdir");
    let sessions_path = temp.path().join("sessions.json");
    let store = JsonFileSessionStore::new(&sessions_path)
        .with_stale_lock_threshold(Duration::from_secs(60));

    let fresh_record = JsonStoreLockRecord {
        owner_pid: std::process::id(),
        created_at_ms: now_ms(),
    };
    fs::write(
        store.lock_path(),
        serde_json::to_vec(&fresh_record).expect("serialize lock record"),
    )
    .expect("write fresh lock file");

    let err = store
        .append_rows(&[SessionPersistenceRow {
            session_id: "s2".to_string(),
            updated_at_ms: 200,
            deleted: false,
            payload: json!({"step":"create"}),
        }])
        .expect_err("fresh lock should block writer");

    assert_eq!(
        err,
        JsonFileSessionStoreError::LockHeld {
            lock_path: store.lock_path().display().to_string(),
        }
    );
}

#[test]
fn phase5_json_store_does_not_evict_aged_lock_with_live_owner() {
    let temp = tempdir().expect("tempdir");
    let sessions_path = temp.path().join("sessions.json");
    let store = JsonFileSessionStore::new(&sessions_path)
        .with_stale_lock_threshold(Duration::from_millis(10));

    let aged_live_record = JsonStoreLockRecord {
        owner_pid: std::process::id(),
        created_at_ms: now_ms().saturating_sub(10_000),
    };
    fs::write(
        store.lock_path(),
        serde_json::to_vec(&aged_live_record).expect("serialize aged live lock record"),
    )
    .expect("write aged lock file");

    let err = store
        .append_rows(&[SessionPersistenceRow {
            session_id: "s2-live".to_string(),
            updated_at_ms: 250,
            deleted: false,
            payload: json!({"step":"create"}),
        }])
        .expect_err("aged lock with live owner should block writer");

    assert_eq!(
        err,
        JsonFileSessionStoreError::LockHeld {
            lock_path: store.lock_path().display().to_string(),
        }
    );
    assert!(
        store.lock_path().exists(),
        "live-owner lock file should not be evicted"
    );
}

#[test]
fn phase5_bigquery_adapter_process_path_and_dedup_tombstone_semantics() {
    let temp = tempdir().expect("tempdir");
    let script = temp.path().join("fake-bq.sh");
    let insert_log = temp.path().join("insert.log");
    let insert_args_log = temp.path().join("insert-args.log");
    let query_result = temp.path().join("query-result.json");
    let query_args_log = temp.path().join("query-args.log");
    build_fake_bq_script(
        &script,
        &insert_log,
        &insert_args_log,
        &query_result,
        &query_args_log,
    );

    let writes = vec![
        SessionPersistenceRow {
            session_id: "s1".to_string(),
            updated_at_ms: 1_000,
            deleted: false,
            payload: json!({"step":"create"}),
        },
        SessionPersistenceRow {
            session_id: "s1".to_string(),
            updated_at_ms: 2_000,
            deleted: true,
            payload: json!({}),
        },
        SessionPersistenceRow {
            session_id: "s2".to_string(),
            updated_at_ms: 1_500,
            deleted: false,
            payload: json!({"step":"create"}),
        },
        SessionPersistenceRow {
            session_id: "s2".to_string(),
            updated_at_ms: 3_000,
            deleted: false,
            payload: json!({"step":"update","version":2}),
        },
    ];
    fs::write(
        &query_result,
        serde_json::to_string(&writes).expect("serialize query rows"),
    )
    .expect("write query rows");

    let store = BigQuerySessionStoreAdapter::new(&script, "phase5_dataset", "phase5_table")
        .with_project_id("phase5-project");

    store
        .stream_insert_rows(&writes)
        .expect("fake bq insert command should succeed");
    let latest = store.read_latest_rows().expect("query latest rows");
    let live = store.read_live_rows().expect("query live rows");

    let insert_args = fs::read_to_string(&insert_args_log).expect("read insert args");
    let insert_payload = fs::read_to_string(&insert_log).expect("read insert payload");
    let query_args = fs::read_to_string(&query_args_log).expect("read query args");
    assert!(insert_args.contains("insert phase5_dataset.phase5_table"));
    assert!(insert_args.contains("--project_id=phase5-project"));
    assert!(insert_payload.contains("\"session_id\":\"s1\""));
    assert!(query_args.contains("SELECT session_id, updated_at_ms, deleted, payload"));

    assert_eq!(
        latest,
        vec![
            SessionPersistenceRow {
                session_id: "s1".to_string(),
                updated_at_ms: 2_000,
                deleted: true,
                payload: json!({}),
            },
            SessionPersistenceRow {
                session_id: "s2".to_string(),
                updated_at_ms: 3_000,
                deleted: false,
                payload: json!({"step":"update","version":2}),
            },
        ]
    );
    assert_eq!(
        live,
        vec![SessionPersistenceRow {
            session_id: "s2".to_string(),
            updated_at_ms: 3_000,
            deleted: false,
            payload: json!({"step":"update","version":2}),
        }]
    );
}
