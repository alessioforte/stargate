# Temporary dependency patches

## sqlx-sqlite 0.9.0

`sqlx-sqlite/` contains the released crates.io 0.9.0 source, including its
MIT/Apache licenses and source revision metadata. The archive checksum is
recorded in the root [version catalog](../VERSIONS.md). Generated upstream
`Cargo.lock` and `Cargo.toml.orig` are omitted; all Rust source files are
unchanged.

The dependency change is the `libsqlite3-sys` requirement, from
`>=0.30.1, <0.38.0` to `>=0.39.1, <0.40.0`. This admits SQLite 3.54.1 and
prevents resolving an older vulnerable bundled SQLite. SQLx has already
[raised the upper bound upstream](https://github.com/transact-rs/sqlx/commit/10462fa4414c6c7f1ed1d39bb31678e3b726878c),
but no crates.io SQLx release containing that change is available yet.

This fixes [RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
without suppressing the advisory or changing the PostgreSQL driver. The root
`[patch.crates-io]` applies the same driver to SQLx and its macro dependencies.
The driver is excluded from workspace membership; the application and DB tests
exercise it through the normal edge feature.

The manifest also allows the released driver's existing `time` API deprecation
warnings. Cargo normally caps registry dependency lints; using a path exposes
them to the CI's global `-D warnings`. This exception is limited to the vendored
driver's `deprecated` lint. Application lint settings and the security audit
remain unchanged.

Remove the path patch, workspace exclusion and this vendored driver after
adopting a released SQLx version that permits `libsqlite3-sys >=0.39.1`. Update
the lockfile/catalog and rerun audit plus both backend test suites.
