# Temporary dependency pins

Cargo package versions/checksums remain in `Cargo.lock`; workspace requirements
remain in `Cargo.toml`. This catalog records the temporary security patch that
cannot yet use a released SQLx dependency. Chart image pins are maintained
separately in [the chart catalog](k8s/stargate/VERSIONS.md).

| Component | Version | Integrity / reason |
| --- | --- | --- |
| sqlx-sqlite source | 0.9.0 | crates.io archive SHA-256 `488e99c397a62007e4229aec669a179816339afc6d2620ca6fa420dbee2e982c`; unchanged Rust source, dependency constraint patched locally |
| libsqlite3-sys | 0.39.1 | Bundled SQLite 3.54.1, fixes RUSTSEC-2026-0459; registry checksum in Cargo.lock |
| Upstream compatibility change | `10462fa4414c6c7f1ed1d39bb31678e3b726878c` | SQLx admits libsqlite3-sys 0.39.x; [patch details and removal condition](vendor/README.md) |
