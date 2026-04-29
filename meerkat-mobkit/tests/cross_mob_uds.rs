//! Cross-mob UDS transport plumbing — Phase 1 surface tests.
//!
//! Mirrors `cross_mob_tcp.rs` but for Unix domain sockets. See that file
//! for the contract narrative.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args
)]

use std::sync::Arc;

use meerkat_client::TestClient;
use meerkat_mob::MobDefinition;

use meerkat_mobkit::UnifiedRuntimeBuilder;
use meerkat_mobkit::contact_directory::{ContactDirectory, MobTransport};
use meerkat_mobkit::runtime::cross_mob_remote::RemoteMobError;
use meerkat_mobkit::unified_runtime::cross_mob::{CrossMobError, build_uds_peer_spec};

const MINIMAL_MOB_TOML_A: &str = r#"
[mob]
id = "mob-a"

[profiles.worker]
model = "test-model"
"#;

const MINIMAL_MOB_TOML_B: &str = r#"
[mob]
id = "mob-b"

[profiles.worker]
model = "test-model"
"#;

fn definition_a() -> MobDefinition {
    MobDefinition::from_toml(MINIMAL_MOB_TOML_A).expect("parse mob-a")
}
fn definition_b() -> MobDefinition {
    MobDefinition::from_toml(MINIMAL_MOB_TOML_B).expect("parse mob-b")
}

#[tokio::test]
async fn uds_contact_directory_round_trips_through_toml() {
    let dir = ContactDirectory::from_toml(
        r#"
        [mobs]
        mob-b = "uds:///var/run/meerkat/mob-b.sock"
        "#,
    )
    .expect("parse uds contact directory");

    let entry = dir.get("mob-b").expect("mob-b entry");
    assert_eq!(
        entry.transport,
        MobTransport::Uds("/var/run/meerkat/mob-b.sock".to_string())
    );
}

#[tokio::test]
async fn uds_peer_spec_helper_emits_triple_slash_address() {
    // `uds:///path` — the comms-layer parser splits on `://` and treats
    // the remainder as an absolute filesystem path. The helper accepts
    // either `/path` or `path` and normalizes.
    let spec = build_uds_peer_spec(
        "mob-a/worker/alice",
        "00000000-0000-4000-8000-000000000001",
        "/var/run/meerkat/mob-a.sock",
    )
    .expect("uds spec");
    assert_eq!(spec.address.endpoint(), "/var/run/meerkat/mob-a.sock");
}

#[tokio::test]
async fn unified_runtime_with_uds_contact_reports_remote_contacts() {
    let tmp_a = tempfile::tempdir().expect("temp dir a");
    let tmp_b = tempfile::tempdir().expect("temp dir b");
    let path_a = tmp_a.path().join("mob-a.sock");
    let path_b = tmp_b.path().join("mob-b.sock");

    let dir_a = ContactDirectory::from_toml(&format!(
        r#"
        [mobs]
        mob-b = "uds://{}"
        "#,
        path_b.display(),
    ))
    .expect("dir for mob-a");
    let dir_b = ContactDirectory::from_toml(&format!(
        r#"
        [mobs]
        mob-a = "uds://{}"
        "#,
        path_a.display(),
    ))
    .expect("dir for mob-b");

    let rt_a = UnifiedRuntimeBuilder::default()
        .definition(definition_a())
        .default_llm_client(Arc::new(TestClient::default()))
        .contact_directory(dir_a)
        .build()
        .await
        .expect("build mob-a");
    let rt_b = UnifiedRuntimeBuilder::default()
        .definition(definition_b())
        .default_llm_client(Arc::new(TestClient::default()))
        .contact_directory(dir_b)
        .build()
        .await
        .expect("build mob-b");

    assert!(rt_a.has_contact_directory());
    assert!(rt_a.has_remote_contacts());
    assert!(!rt_a.has_inproc_contacts());
    assert!(rt_b.has_remote_contacts());

    drop(rt_a);
    drop(rt_b);
}

#[tokio::test]
async fn wire_cross_mob_over_uds_surfaces_remote_seam() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let sock = tmp.path().join("mob-b.sock");

    let dir = ContactDirectory::from_toml(&format!(
        r#"
        [mobs]
        mob-b = "uds://{}"
        "#,
        sock.display(),
    ))
    .expect("dir");
    let rt = UnifiedRuntimeBuilder::default()
        .definition(definition_a())
        .default_llm_client(Arc::new(TestClient::default()))
        .contact_directory(dir)
        .build()
        .await
        .expect("build");

    let err = rt
        .wire_cross_mob("alice", "bob", "mob-b")
        .await
        .expect_err("phase 1: uds wire surfaces the remote seam");
    match err {
        CrossMobError::Remote(RemoteMobError::ControlChannelUnavailable {
            mob_id,
            endpoint,
            ..
        }) => {
            assert_eq!(mob_id, "mob-b");
            assert!(
                endpoint.starts_with("uds:///"),
                "endpoint should carry uds scheme, got {endpoint}"
            );
            assert!(
                endpoint.contains("mob-b.sock"),
                "endpoint should preserve socket path, got {endpoint}"
            );
        }
        other => panic!("expected ControlChannelUnavailable, got {other:?}"),
    }

    drop(rt);
}
