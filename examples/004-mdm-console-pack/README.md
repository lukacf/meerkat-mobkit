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

Important: target `--listen HOST:PORT` is the bind address. Target
`--advertise tcp://HOST:PORT` is the address the console host dials. The GCP
helper defaults to binding `0.0.0.0:5791` and advertising the VM internal IP,
which is right for a VPN/VPC-reachable console host. Pass `--advertise` if your
network path is different.

Meerkat validates the target's binding address against the target comms
runtime's advertised listener during the remote bind handshake. For SSH
loopback tunnels, use one stable port per target and keep the target's
`--listen` and `--advertise` host/port aligned:

```bash
./scripts/gcp-target.sh start \
  --id target-b \
  --name gcp-target-b \
  --advertise tcp://127.0.0.1:5792

gcloud compute ssh mobkit-mdm-target-target-b -- -N \
  -L 127.0.0.1:5792:127.0.0.1:5792 \
  -R 127.0.0.1:5790:127.0.0.1:5790 \
  -R 127.0.0.1:5793:127.0.0.1:5793
```

The helper infers `--listen 127.0.0.1:5792` from that `--advertise` value. If
you need to advertise a public DNS name or a NAT address that the VM cannot bind
directly, pass both `--listen` and `--advertise` and make sure the resulting
binding address is the one Meerkat should validate.

For cross-host targets, the console's Meerkat supervisor bridge must also be
reachable from the targets. The hive/local members also need a routable comms
address so target peer responses can get back to the console process:

```bash
MDM_SUPERVISOR_BIND_ADDRESS=0.0.0.0:5790 \
MDM_SUPERVISOR_ADVERTISED_ADDRESS=tcp://<console-reachable-host>:5790 \
MDM_AGENT_COMMS_ADDRESS=<console-reachable-host>:5793 \
./004-mdm-console-pack/scripts/start-console.sh
```

For a local-only demo, the default supervisor bridge is
`tcp://127.0.0.1:5790` and the default local member comms listener is
`tcp://127.0.0.1:5793`. For SSH loopback tunnels, keep
`MDM_AGENT_COMMS_ADDRESS=127.0.0.1:5793` and include the reverse tunnel above.

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

## Smoke

```bash
cd examples
npm run mdm:smoke
npm run mdm:browser-smoke
npm run mdm:real-target-smoke
npm run mdm:local-target
npm run mdm:console
```

The empty-target smokes verify that the console boots without the old kennel
path. `mdm:real-target-smoke` starts a disposable real `mdm_mob_target`, binds it
as an external mob member, sends a queued mob turn to that target, and checks
that the target process observed the peer turn. That is the release gate for the
Meerkat bridge integration.

The pack is pinned to the Meerkat 0.6.32 family. Re-apply and validate the pin
with:

```bash
cd examples
npm run mdm:upgrade-meerkat -- 0.6.32
npm run mdm:real-target-smoke
```

Full operator validation requires at least one local or remote target runtime
with unrestricted shell tools enabled on the target side. The useful prompt is
something like: "Ask every target what machine it is running on." The answer
should come from target-side peer turns, not from roster labels.

`mdm:real-target-smoke`, `mdm:hive-target-smoke`, and `mdm:resume-smoke` are the
release gates for the vendored Meerkat 0.6.32 bridge patch set in this branch.
If they fail, treat that as a real bridge or MobKit regression rather than
falling back to labels, demo model text, or static binding metadata.

On small GCP VMs the target helper builds `mdm_mob_target` with incremental
artifacts and debug info disabled to avoid filling the default 10 GB boot disk.
