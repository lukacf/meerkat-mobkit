# MDM Mob Target Deployment

This note describes the deployment model for the MobKit MDM console example in
`examples/004-mdm-console-pack`.

## Roles

- Console host: runs MobKit, the hive member, the browser console, persistent
  MobKit state, and the externally reachable supervisor bridge.
- Target runtime: runs `mdm_mob_target` on each managed machine. It owns local
  credentials, sessions, shell/tool execution, comms identity, and state.
- Meerkat/MobKit mob: owns peer wiring between hive and targets. Remote target
  members are ordinary external mob members, not proxy records.

The console host can ask targets to do work, but targets perform work locally.
The console host must never synthesize target-machine answers from labels,
bindings, or deployment metadata.

## Network Shape

Each target exposes a signed Meerkat comms TCP endpoint:

```text
target bind:       0.0.0.0:5791 or 10.x.y.z:5791
target advertised: tcp://<console-reachable-target>:5791
```

The console host exposes the Meerkat mob supervisor bridge:

```text
console bind:       0.0.0.0:5790 or 127.0.0.1:5790
console advertised: tcp://<target-reachable-console>:5790
```

For single-machine demos, both sides can use loopback. For GCP/VPC demos, the
advertised addresses must be routable between the console host and the target
VMs, and firewall rules must allow the chosen ports.

## Local Development

From `examples/`:

```bash
npm install
./004-mdm-console-pack/scripts/local-target.sh start --id target-a --name this-mac
./004-mdm-console-pack/scripts/start-console.sh
```

The local helper builds and starts `mdm_mob_target`, waits for a binding file,
and merges it into `.state/target-bindings.json`.

## GCP Target

From `examples/004-mdm-console-pack`:

```bash
cp deploy/gcp.env.example .env.gcp
$EDITOR .env.gcp
./scripts/gcp-target.sh start --id target-b --name gcp-target-b
```

The helper creates or reuses the VM, syncs this checkout, forwards selected
provider credential environment variables, builds `mdm_mob_target` on the VM,
starts it, fetches the binding, and merges the binding into the console target
file.

Then start the console with a supervisor bridge address reachable from the VM:

```bash
MDM_SUPERVISOR_BIND_ADDRESS=0.0.0.0:5790 \
MDM_SUPERVISOR_ADVERTISED_ADDRESS=tcp://<console-reachable-host>:5790 \
./scripts/start-console.sh
```

## Current Upstream Gate

The deployment helpers now create real target runtimes and real MobKit external
member bindings. Meerkat 0.6.29 still has bridge reply gaps. The currently
observed published-crate failure is that the supervisor side decodes a typed
`BridgeReply` as a bare bind payload:

```text
failed to decode bridge command response: unknown field `result`
```

Earlier local validation also exposed the live TCP reply-route gap: in the real
`mdm_mob_target` path, the target receives the supervisor `BindMember` request
and then fails to send the response because the supervisor sender peer is not
present in the target comms trust router.

PR `lukacf/meerkat#746` fixes that production trust repair. When the fix is
published as Meerkat 0.6.30, cut over from `examples/` with:

```bash
npm run mdm:upgrade-meerkat -- 0.6.30
npm run mdm:real-target-smoke
```

Run this from the repository root to reproduce the current gate:

```bash
npm --prefix examples run mdm:real-target-smoke
```

Expected failures until Meerkat 0.6.30:

```text
failed to decode bridge command response: unknown field `result`
supervisor request '<uuid>' timed out after 30000ms
comms_drain: failed to send bridge response ... error=peer not found: <supervisor-peer-id>
```

After the upstream fix, the same script is the acceptance check for local target
bind plus delivery of a queued mob turn to the target. The GCP helper uses the
same target binary and binding path, so it exercises the same mechanics across
hosts once the console supervisor advertised address is routable.

## Production Hardening

This example is suitable for validating the real remote mob path. Before using
it as production MDM, add:

- authenticated/TLS transport for target and supervisor ports;
- restricted firewall rules between console and target subnets;
- systemd/launchd installers for `mdm_mob_target`;
- durable state backup for target identities and session state;
- audit-log retention outside the demo `.state/` directory.
