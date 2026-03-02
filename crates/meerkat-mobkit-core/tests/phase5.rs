use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use meerkat_mobkit_core::{
    BigQuerySessionStoreAdapter, JsonFileSessionStore, JsonFileSessionStoreError,
    JsonStoreLockRecord, SessionPersistenceRow,
};
use serde_json::json;
use tempfile::tempdir;

#[path = "support/bigquery_http_mock.rs"]
mod bigquery_http_mock;

use bigquery_http_mock::{MockHttpResponse, MockHttpServer};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
    let query_rows = serde_json::json!({
        "jobComplete": true,
        "rows": [
            {"f":[{"v":"s1"},{"v":"1000"},{"v":"false"},{"v":"{\"step\":\"create\"}"}]},
            {"f":[{"v":"s1"},{"v":"2000"},{"v":"true"},{"v":"{}"}]},
            {"f":[{"v":"s2"},{"v":"1500"},{"v":"false"},{"v":"{\"step\":\"create\"}"}]},
            {"f":[{"v":"s2"},{"v":"3000"},{"v":"false"},{"v":"{\"step\":\"update\",\"version\":2}"}]}
        ]
    });
    let mock_server = MockHttpServer::start(vec![
        MockHttpResponse::json(serde_json::json!({})),
        MockHttpResponse::json(query_rows.clone()),
        MockHttpResponse::json(query_rows),
    ]);

    let store = BigQuerySessionStoreAdapter::new_native("phase5_dataset", "phase5_table")
        .with_project_id("phase5-project")
        .with_access_token("phase5-test-token")
        .with_api_base_url(format!("{}/bigquery/v2", mock_server.base_url()));

    store
        .stream_insert_rows(&writes)
        .expect("insertAll should succeed");
    let latest = store.read_latest_rows().expect("query latest rows");
    let live = store.read_live_rows().expect("query live rows");
    let requests = mock_server.captured_requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        requests[0].path,
        "/bigquery/v2/projects/phase5-project/datasets/phase5_dataset/tables/phase5_table/insertAll"
    );
    let first_body: serde_json::Value =
        serde_json::from_str(&requests[0].body).expect("parse insert request");
    let insert_rows = first_body["rows"]
        .as_array()
        .expect("insert request rows array");
    assert_eq!(insert_rows.len(), writes.len());
    assert_eq!(requests[1].method, "POST");
    assert_eq!(
        requests[1].path,
        "/bigquery/v2/projects/phase5-project/queries"
    );
    let query_body: serde_json::Value =
        serde_json::from_str(&requests[1].body).expect("parse query request");
    assert!(query_body["query"]
        .as_str()
        .expect("query text")
        .contains("SELECT session_id, updated_at_ms, deleted, payload"));

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
