# MDM Kennel And Target Deployment

This note describes the deployment model for the MobKit MDM console example in
`examples/004-mdm-console-pack`.

## Roles

- Kennel: central control plane. It accepts target registration and heartbeat,
  owns operator leases, writes the generated MobKit contact directory, and hosts
  the MobKit console adapter.
- Target daemon: per-machine control plane. It owns local execution, sessions,
  model choice, process lifecycle, and peer key material.
- MobKit console: operator surface. It projects the kennel fleet as a roster,
  topology, health, logs, chat, timeline, and target actions.

The important boundary is that the kennel can decide which target receives work,
but the target performs work locally. The kennel does not impersonate target
state or keep a shadow terminal session.

## Network Shape

Targets expose a small HTTP control endpoint:

- `GET /health`
- `GET /mdm/info`
- `GET /control/sessions`
- `GET /control/peer_pubkey`
- `POST /control/turn`
- `POST /control/interrupt`
- `POST /control/respawn`
- `POST /control/model`

Legacy targets may also expose `POST /legacy/turn`. The kennel prefers
`/control/turn` for control-capable targets and only falls back to legacy for
control targets when that first call fails.

Targets bind locally using `--listen`, and separately advertise the
kennel-reachable URL using `--advertise-url`. This matters in containers and
VMs: a target can listen on `0.0.0.0:5792` but must register a dialable address
such as `http://target-b:5792` inside Docker or `http://10.x.y.z:5792` in a VPC.

## Kennel Flow

1. Target starts and loads or creates its stable peer key in its state directory.
2. Target posts registration to `POST /api/register` on the kennel.
3. Target heartbeats to `POST /api/heartbeat`.
4. Kennel projects the target into MobKit as a target proxy identity.
5. Operator claims a lease before sending intrusive work.
6. Console tool calls go through the kennel to the target control endpoint.
7. Kennel rewrites `contacts.generated.toml` whenever target registration or
   peer key data changes.

## Local Development

Use the example pack from the repo root:

```bash
npm --prefix examples run mdm:smoke
npm --prefix examples run mdm:browser-smoke
npm --prefix examples run mdm:docker-smoke
```

The local smoke starts the kennel, two loopback target daemons, and the MobKit
console. The browser smoke opens the real console. The Docker smoke starts the
kennel and target daemons in separate containers, then sends a remote shell turn
through the kennel to a target.

## Docker Deployment

The included Compose file models the default production topology:

- `kennel-console` listens on `0.0.0.0:5788`.
- `target-a` registers as a legacy-capable remote target and advertises
  `http://target-a:5791`.
- `target-b` registers as a control-capable remote target and advertises
  `http://target-b:5792`.

The kennel stores state under `.state/` in the example pack. Target containers
store demo state under `/tmp/mdm-target-*`; production targets should use a
durable host path such as `/var/lib/mdm-targetd`.

## VM Deployment

For a VM fleet:

1. Run one central kennel reachable from the target VPC.
2. Install `mdm-targetd` on each VM using systemd or launchd.
3. Bind targetd to a local interface, for example `0.0.0.0:5792`.
4. Advertise the VPC-reachable address with `--advertise-url`.
5. Persist target state under `/var/lib/mdm-targetd`.
6. Back up kennel state, generated contacts, and target identity state.

The optional GCP helper scripts are intentionally `.env.gcp`-driven. They do not
commit project IDs, zones, keys, or private infrastructure values.

For a current-worktree smoke, `scripts/gcp-smoke.sh` creates one kennel VM and
`GCP_TARGET_COUNT` target VMs, syncs the local checkout to `/opt/meerkat-mobkit`,
starts the kennel with bearer-token auth, starts target daemons on each target
VM, sends a remote shell marker through the kennel, and leaves cleanup to
`scripts/gcp-cleanup-targets.sh`.

## Production Hardening

This example includes the production hooks needed to deploy the shape:

- Optional HTTPS serving for the kennel with `MDM_TLS_CERT_PATH` and
  `MDM_TLS_KEY_PATH`.
- Bearer-token auth for kennel APIs and target control endpoints.
- Persisted kennel state, generated contacts, and MobKit state under `.state/`.
- Backup packaging through `scripts/backup-kennel-state.sh`.
- Systemd and launchd target installer helpers.
- A central kennel systemd unit template.

Before treating this as real production MDM, the remaining hardening is:

- Mutual auth or signed target registration instead of shared bearer tokens.
- Lease renewal and stricter expiry enforcement around intrusive actions.
- Audit-log persistence outside demo state.
- Installer packaging that wires systemd/launchd from stable release artifacts
  instead of cloning the development repository.
- Firewall rules that expose target control endpoints only to the kennel.
