# Reference intent atlas

This project owns this small atlas to exercise the contract between cmf and cmv.
Production atlases have separate owners. The two binaries only read atlas
records, sensors, profiles, and validators.

The `rust-shipping` profile selects three Rust intents. Two have executable
validators: isolation fails when `src/violation.marker` exists, and the gateway
boundary passes when the project has a `Cargo.toml`. The documentation intent
has no validator, so a strict check reports it as unchecked. The Python record
and sensor exercise ecosystem exclusion without changing the Rust profile.

`cases/compliant` and `cases/violation` are small source workspaces. The
adoption test copies the atlas into a temporary git repository,
uses an isolated project and home, and runs the real cmf and cmv binaries. It
checks preview purity, Codex installation, manifest and pin data, verdicts
before and after a source correction, strict unchecked evidence, and a moved
atlas HEAD. The golden and malformed-input fixtures under `cmf/tests` and
`cmv/tests` stay separate because they pin different compatibility behavior.

Run the loop from the repository root:

```sh
cargo build -p cmf -p cmv
cargo test -p cmv --test adoption_loop
```
