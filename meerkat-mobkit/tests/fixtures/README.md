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
