Frozen released-corpus fixtures. Never regenerate these with current code:
current writers cannot and must not mint released envelopes, and a
fixture re-synthesized by the pinned writer silently passes exactly the
writer-drift bugs these tests exist to catch (the 0.8.11 fleet-import
regression shipped past a synthetic 26-chain test for that reason).

- v0_8_10_released_session.json - an exact meerkat 0.8.10-written session
  envelope (v2), copied from meerkat-core/tests/fixtures/
  v0_8_10_ob3_recovery_migration_session.json at pin 2bd60a1a. Used by the
  adapter import-on-load regression.

- v0_8_10_zero_rewrite_supervisor_session.json - a REAL released-minted
  0.8.10 mob-supervisor snapshot (zero-rewrite transcript graph: one
  revision, no commits key on the wire, singleton live-head body equal to
  the live transcript), extracted from the HomeCore forensic bundle
  (rows-019f2bdc, runtime_session_snapshots, session
  019f2bdc-a781-7060-bff8-0b97b7a4fcee). Used by the zero-rewrite
  import-on-load regression pinning the meerkat d6cafd405 acceptance and
  its strictness.

- released_0_8_8_realms/ - four COMPLETE realms (continuity.sqlite3 v2 +
  runtime.sqlite v1 + mobkit satellites + WAL/SHM/.mfence sidecars, no
  realm_manifest.json) minted by the PUBLIC-RELEASE mobkit 0.8.8
  rpc_gateway (embedding meerkat SDK 0.8.10) via the stdin-JSONL capture
  driver (capture_0810_fixture.py, job 6b689d90). Consumed by
  tests/released_realm_upgrade_drive.rs, which boots the CURRENT gateway
  over a staged copy. Never regenerate with current code, and never open
  these files with sqlite in place: a read/write open checkpoints and
  truncates the WAL, destroying the released byte shape (the test pins the
  continuity main/WAL sizes and fails loudly). The empty blobs/ directory
  is part of the released shape; git cannot track it, so the test's
  staging helper restores it.
    - baseline/       multi-boot clean-shutdown history, 5 turns
                      (head: 11 messages)
    - burst_drain/    pipelined 6-send burst fully drained by
                      mobkit/shutdown (head: 21 messages)
    - crash_sigkill/  SIGKILLed after the burst drained: un-checkpointed
                      WALs, no shutdown attestation; all 10 inputs consumed
                      and the legacy runtime snapshot in sync with the head
                      (the kill landed at idle)
    - deploy_cycles/  4 boot/turn/clean-shutdown deploy cycles (head: 9
                      messages); rewrite_count is 0 - the released binary
                      minted no resume rewrites for an unchanged system
                      prompt
