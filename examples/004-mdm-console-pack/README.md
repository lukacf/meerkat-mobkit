# 004 - MDM Console Pack

This pack re-imagines the old terminal MDM flow as a MobKit console
deployment. It is independent of the Meerkat `035-mdm-tux-rs` example.
See `../../docs/design/mdm-kennel-target-deployment.md` for the kennel/target
deployment model.

## Local Smoke

```bash
cd examples
npm install
npm run mdm:smoke
npm run mdm:auth-smoke
npm run mdm:browser-smoke
```

The local smoke starts a kennel API, two remote target daemons on loopback, a
MobKit console runtime, claims a target, runs a remote shell marker, verifies
generated contacts, and checks `/console/experience`.
The auth smoke runs the same target path with bearer-token auth enabled and
verifies unauthenticated kennel API calls are rejected.

## Run The Console

```bash
cd examples
./004-mdm-console-pack/examples.sh --run
```

Open the printed `/console` URL. The target daemons are separate processes and
all target turns go through the kennel to target-owned HTTP endpoints.

## Docker Remote Targets

```bash
cd examples/004-mdm-console-pack
docker compose up
```

The compose file runs a central kennel-console service plus two target
containers. Targets bind inside their own container and advertise service-DNS
URLs back to the kennel, so the kennel never tries to dial a target's loopback
address. To run the API-level container smoke:

```bash
./examples.sh --docker-smoke
```

GCP is optional and not required for normal development.

## Optional GCP

Copy `deploy/gcp.env.example` to `.env.gcp` and fill it locally. The scripts
only read local environment files and do not commit project IDs, zones, keys, or
other private infrastructure values. The installer advertises each VM's first
private IP address to the kennel; the kennel must be able to reach TCP/5792 on
that address for remote turns to work.

```bash
./scripts/gcp-create-targets.sh
./scripts/gcp-install-targetd.sh
./scripts/gcp-smoke.sh
./scripts/gcp-cleanup-targets.sh
```

`gcp-smoke.sh` creates a kennel VM and target VMs, syncs the current checkout,
runs a remote target turn, and expects you to run cleanup afterwards. The script
uses only the ignored `.env.gcp` file for project, zone, prefix, and token
values.

## Production State

The kennel writes `kennel-state.json`, `contacts.generated.toml`, and MobKit
SQLite state under `.state/` by default. Use:

```bash
./scripts/backup-kennel-state.sh
```

Systemd and launchd target installers live under `scripts/`. They copy a local
env file into `/etc/mdm-targetd`; keep that env file local because it contains
the kennel and target bearer tokens.
