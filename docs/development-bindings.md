# Development source bindings

MobKit normally resolves the exact Meerkat version pinned in
`meerkat-mobkit/Cargo.toml`. During coordinated source development before a
matching crate release, configure an explicit local binding instead of editing
that production manifest:

```sh
scripts/dev-bindings configure --meerkat /path/to/meerkat
MEERKAT_DEV_BINDINGS=1 scripts/repo-cargo check --workspace --all-targets
scripts/dev-bindings verify
```

The command generates an ignored `.dev-bindings/cargo-config.toml` from
Meerkat's canonical crate map. `scripts/repo-cargo` loads it only when
`MEERKAT_DEV_BINDINGS=1`; release commands remain on crates.io. Bound commands
use a separate ignored lockfile and restore the production `Cargo.lock` on
success, failure, or interruption.

`scripts/dev-bindings clear` removes the local binding. This bridge is
temporary and should be removed from CI once Meerkat 0.8.0 is published and
MobKit can move its normal crate pins forward.
