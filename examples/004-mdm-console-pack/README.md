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

```bash
cd examples
npm install
./004-mdm-console-pack/examples.sh --run --targets ./004-mdm-console-pack/target-bindings.json
```

Open the printed `/console` URL. The roster should show `Hive` plus the remote
targets from the binding file. Asking the hive to query hardware should produce
peer messages to the target members; if the timeline only shows local metadata,
that is a MobKit/Meerkat remote-support gap.

## Smoke

```bash
cd examples
npm run mdm:smoke
npm run mdm:browser-smoke
```

These local smokes only verify that the console boots without the old kennel
path. They do not prove remote execution. Live validation requires at least one
local target runtime and one remote target runtime with unrestricted shell tools
enabled on the target side.
