# 004 - MDM Console Pack

This pack re-imagines the old TUX MDM flow as a MobKit console over one real
mob roster. There is no kennel service and no target HTTP registry in this
design. MobKit owns the roster, the hive is a local member, and every target is
declared as a remote `backend: external` mob member with a real Meerkat comms
address and Ed25519 public key.

The important test is whether hive-to-target traffic is actual peer/comms
traffic. A roster label saying `platform=linux-gcp-vm` is not a target answer.

## Target Bindings

Run or provision target agents separately, then pass their bindings to the
console:

```json
[
  {
    "id": "target-a",
    "name": "this-mac",
    "site": "local",
    "platform": "darwin-local",
    "address": "tcp://127.0.0.1:5791",
    "public_key": "ed25519:...",
    "bootstrap_token": "..."
  }
]
```

The same value can be supplied as `binding` in the canonical MobKit/Meerkat
wire shape:

```json
{
  "kind": "external",
  "address": "tcp://127.0.0.1:5791",
  "identity": {
    "kind": "ed25519_public_key",
    "public_key": "ed25519:..."
  },
  "bootstrap_token": "..."
}
```

See `target-bindings.example.json` for the file shape. The example keys are not
live credentials.

## Run

For a local target on this machine:

```bash
cd examples
npm install
./004-mdm-console-pack/scripts/local-target.sh start --id target-a --name this-mac
./004-mdm-console-pack/scripts/start-console.sh
```

The local target helper starts the real `mdm_mob_target` Rust example, waits
until it writes a binding, and upserts that binding into
`004-mdm-console-pack/.state/target-bindings.json`.

For a GCP target:

```bash
cd examples/004-mdm-console-pack
cp deploy/gcp.env.example .env.gcp
$EDITOR .env.gcp
./scripts/gcp-target.sh start --id target-b --name gcp-target-b
./scripts/start-console.sh
```

`gcp-target.sh` creates the VM if needed, syncs the repo, forwards provider
credential env vars such as `OPENAI_API_KEY`, starts the same real target
runtime on the VM, fetches its binding, and merges it into the console target
file.

Important: `--listen HOST:PORT` is both the bind address and the advertised
MobKit address. The console host must be able to reach that address. The GCP
helper defaults to the VM's internal IP, which is right for a VPN/VPC-reachable
operator machine. Pass `--listen <reachable-ip>:5791` if your network path is
different.

To run with a pre-existing binding file:

```bash
cd examples
npm install
./004-mdm-console-pack/examples.sh --run --targets ./004-mdm-console-pack/target-bindings.json
```

Open the printed `/console` URL. The roster should show `Hive` plus the remote
targets from the binding file. Asking the hive to query hardware should produce
peer messages to the target members; if the timeline only shows local metadata,
that is a MobKit/Meerkat remote-support gap.

The success signal is not "target is listed." The success signal is a target
turn that runs on the target host, uses target-side tools or shell where
appropriate, and returns over the MobKit/Meerkat peer path.

## Current Remote Gap

Live binding currently reaches the target-side comms drain, but Meerkat's mob
supervisor bridge still advertises its supervisor as an in-process address
(`inproc://<mob>/__mob_supervisor__`). A remote target can accept `BindMember`
and publish supervisor trust, but it cannot route the bridge response back to
that in-process supervisor. The observed failure is a target-side
`failed to send bridge response ... peer not found` followed by a MobKit
`supervisor request ... timed out`.

That is the next real remote-support gap to close in Meerkat/MobKit before a
GCP target can complete the full bind-and-turn loop.

## Smoke

```bash
cd examples
npm run mdm:smoke
npm run mdm:browser-smoke
npm run mdm:local-target
npm run mdm:console
```

These local smokes only verify that the console boots without the old kennel
path. They do not prove remote execution. Live validation requires at least one
local target runtime and one remote target runtime with unrestricted shell tools
enabled on the target side.
