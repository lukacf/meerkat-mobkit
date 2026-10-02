//! Terminate paths that the public forced-failure option does not reach:
//! the replacement child of an aborted respawn, and shutdown. Each check runs
//! once, right after the call that releases the process.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::module_process::test_seam;
use super::*;
use crate::types::DiscoverySpec;

fn shell_module(id: &str, script: String) -> ModuleConfig {
    ModuleConfig {
        id: id.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script],
        restart_policy: RestartPolicy::Never,
    }
}

/// Appends its pid before emitting a ready event, then keeps running.
fn long_lived(id: &str, pid_file: &Path) -> ModuleConfig {
    shell_module(
        id,
        format!(
            r#"echo $$ >> '{}'; printf '%s\n' '{{"event_id":"evt-{id}","source":"module","timestamp_ms":1,"event":{{"kind":"module","module":"{id}","event_type":"ready","payload":{{}}}}}}'; exec sleep 30"#,
            pid_file.display()
        ),
    )
}

fn config(module: ModuleConfig) -> MobKitConfig {
    MobKitConfig {
        discovery: DiscoverySpec {
            namespace: "module-process".to_string(),
            modules: vec![module.id.clone()],
        },
        modules: vec![module],
        pre_spawn: vec![],
    }
}

fn pids(pid_file: &Path) -> Vec<u32> {
    std::fs::read_to_string(pid_file)
        .unwrap()
        .lines()
        .map(|line| line.trim().parse().unwrap())
        .collect()
}

fn running_or_unreaped(pid: u32) -> bool {
    Command::new("sh")
        .args(["-c", &format!("kill -0 {pid} 2>/dev/null")])
        .status()
        .unwrap()
        .success()
}

#[test]
fn an_aborted_respawn_reaps_its_replacement_when_terminating_it_fails() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("pids");
    let mut runtime = start_mobkit_runtime_with_options(
        config(long_lived("respawn", &pid_file)),
        vec![],
        Duration::from_secs(5),
        RuntimeOptions::default(),
    )
    .expect("runtime starts");
    let error = {
        let _failing = test_seam::fail_terminations();
        runtime
            .spawn_member("respawn", Duration::from_secs(5))
            .expect_err("the existing child cannot be terminated")
    };
    assert!(format!("{error:?}").contains("failed to terminate replacement child"));
    let started = pids(&pid_file);
    assert_eq!(started.len(), 2, "original and replacement both started");
    assert!(
        !running_or_unreaped(started[1]),
        "replacement {} leaked by the aborted respawn",
        started[1]
    );
    assert!(
        running_or_unreaped(started[0]),
        "existing child stays owned"
    );
    assert_eq!(runtime.shutdown().orphan_processes, 0);
    assert!(!running_or_unreaped(started[0]));
}

#[test]
fn shutdown_reaps_a_child_whose_termination_fails() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("pids");
    let mut runtime = start_mobkit_runtime_with_options(
        config(long_lived("live", &pid_file)),
        vec![],
        Duration::from_secs(5),
        RuntimeOptions::default(),
    )
    .expect("runtime starts");
    let pid = pids(&pid_file)[0];
    let report = {
        let _failing = test_seam::fail_terminations();
        runtime.shutdown()
    };
    assert_eq!(
        report.orphan_processes, 1,
        "the failed termination is reported"
    );
    assert!(!running_or_unreaped(pid), "module {pid} leaked by shutdown");
}
