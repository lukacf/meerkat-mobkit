# MobKit MDM Console Implementation Plan

This plan tracks the independent MobKit-based MDM implementation in
`examples/004-mdm-console-pack`. The pack must not depend on
`/Users/luka/src/meerkat/examples/035-mdm-tux-rs`; it can keep protocol
compatibility ideas, but all code, config, smokes, and deployment helpers live
in this repo.

## 0. Initial plan

- Build a browser-first MDM console pack, not a terminal UI.
- Keep the authority boundary explicit:
  - target machines own local execution, credentials, state, and sessions;
  - the kennel owns discovery, lease state, generated contacts, and the
    operator console surface;
  - MobKit owns roster projection, chat/timeline, topology, identity controls,
    and runtime/console mechanics.
- Use Docker containers as the normal remote-machine smoke target.
- Keep GCP VM smokes optional and `.env`-driven. No project IDs, zones, keys, or
  instance names that reveal private infrastructure should be committed.

## 1. MVP

Goal: keep the current MDM target protocol shape while replacing the TUX client
with a kennel-hosted MobKit console adapter.

- Add `mdm-targetd` demo daemon with a legacy `/legacy/turn` endpoint and
  registration/heartbeat to the kennel.
- Add `mdm-kennel-console` runner that hosts:
  - a small MDM API for registration, leases, target turn proxying, and status;
  - a MobKit runtime with one visible identity per target plus a hive identity;
  - a stock MobKit console configured for MDM operations.
- Proxy target turns through target-specific SDK tools so console chat/timeline
  show the operator action and target result.
- Add TS smoke coverage for registration, claim, legacy turn, generated contact
  output, and console experience projection.

## 2. Proper v1

Goal: move from TUX-shaped RPC compatibility to target-owned MobKit remote
control endpoints.

- Add `/control/*` endpoints on `mdm-targetd` for target info, turn injection,
  session listing, model selection, respawn, interrupt, and peer pubkey.
- Have the kennel prefer `/control/turn` and fall back to `/legacy/turn` only
  when a target is marked legacy.
- Preserve target-owned execution: targetd performs shell/demo work locally and
  returns events/results to the kennel.
- Keep target identities stable across restarts via state-dir persisted key
  metadata.

## 3. Durable v1.5

Goal: make fleet projection and verification durable enough to operate.

- Generate `contacts.generated.toml` from current target registrations.
- Include target peer pubkeys and control addresses in generated contacts.
- Add lease-aware console labels and tools for claim/release/renew.
- Add browser smoke with Playwright against the actual `/console` page.
- Add Docker Compose smoke with target containers registering as remote
  machines.

## 4. Production

Goal: package the demo in a shape that can be deployed without rewriting it.

- Provide launchd and systemd templates for `mdm-targetd`.
- Provide central kennel deployment files and environment templates.
- Keep TLS/auth required for production mode and optional only for local demos.
- Add backup guidance for kennel state, generated contacts, and target state.
- Provide optional GCP VM helper scripts that source `.env.gcp.example`-style
  config and never commit secrets or private infra values.

## Completion evidence

Current evidence collected in this tree:

- `npm --prefix examples run mdm:smoke`
- `npm --prefix examples run mdm:auth-smoke`
- `npm --prefix examples run mdm:browser-smoke`
- `npm --prefix examples run mdm:docker-smoke`
- `./examples/004-mdm-console-pack/scripts/gcp-smoke.sh`
- `./examples/004-mdm-console-pack/scripts/backup-kennel-state.sh`
- `make test`
- `make test-python`
- `npm --prefix sdk/typescript run validate`
- `npm --prefix console run phase0:types`
- `npm --prefix console run phase1:targets`
- `npm --prefix console run smoke`
- `npm --prefix console run e2e:browser`
- TLS startup probe with `run.ts --api-only --require-auth --require-tls`
- `git diff --check`
- Manual browser verification of `/console`
- Optional GCP smoke documented and runnable from ignored `.env.gcp`, with no
  confidential values committed

Direct `scripts/repo-cargo test -p meerkat-mobkit --quiet` is not the canonical
repo gate because `governance_contracts` expects `.rct/spec.yaml`, which is not
present in this worktree. The documented Rust gate is `make test`, which runs
`nextest` with the repo's `not test(governance_contracts)` exclusion.
