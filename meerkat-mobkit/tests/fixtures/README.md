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

- homecore_ledgerv1_closure/ - HomeCore forensic bundle (2026-08-01): the
  byte-lossless continuity closure of the exact fleet session cited in the
  class-2 and class-3 0.8.11 binding verdicts (domain:calendar,
  019fae11-4dd7-7301-9754-67b646603fb3 - the fleet's max-depth 26-rewrite
  chain; gen-20 production continuity byte-copy). JSON encoding: every
  TEXT/BLOB value is lossless base64 {b64,len}, numbers/nulls verbatim,
  per-table column lists; sha256 pinned in checksums.sha256, source DDL in
  continuity-schema.sql. Consumed by
  identity_first_lazy_recall_continuity.rs
  (homecore_rewrite_carrying_closure_adopts_resumes_and_takes_a_turn),
  which reconstitutes it at test time VERBATIM - every row of every table
  through the bundle's own DDL, zero document surgery - and boots the
  harness under the bundle's OWN identity space (mob homecore, profile
  domain, member domain:calendar), so the persisted mob_member_binding and
  comms_name match the booting mob by construction (lead ruling: fix the
  harness, never the bundle). The class-3 property the
  head carries: released envelope version 2, rewrite_count 26, and NONE of
  the current authority fields (graph_prefix / rewrite_prefix /
  message_row_prefix) - a head that structurally cannot authorize a current
  mutation and must be ADOPTED under the import receipt on the first
  projected write.

- homecore_security_idempotency/ - HomeCore forensic bundle (2026-08-01):
  the lossless three-state head+snapshot evolution of domain:security
  (019fae11-4e87-7482-8796-54b2dac1f410) - untouched gen-20 corpus, the row
  after ONE boot of the fixed binary on a fresh seed, and the row after a
  SECOND boot (the exactly-once violation: identical head_revision, same
  length, different bytes). sha256 pinned in checksums.sha256. Consumed by
  identity_first::adapters::tests::homecore_security_boot_drift_is_zero_durable_change,
  which pins that strict head equality SEES the two-boot drift (updated_at +
  the HashSet-ordered tool-visibility Allow arrays, filed upstream as S5)
  while the scoped exact-resave equality reads it as zero durable change.
