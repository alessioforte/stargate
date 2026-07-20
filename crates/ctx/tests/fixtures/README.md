# `ctx` Golden Fixtures

These files freeze the language-neutral version 1 interoperability vectors.

- `*.jwt` contains one compact RS256 token on a single line.
- `*.claims.json` contains the decoded payload for that token.
- `public.pem` and `public.jwk.json` are equivalent verification keys.
- `private.pem` is test-only fixture material and must never be used outside
  tests.

All vectors use:

- issuer `https://auth.example.com/internal-context`;
- audience `urn:stargate:service:orders`;
- key id `stargate-internal-test`;
- issue time `1784474100` and expiry `1784474130`; and
- dispatch id `01JZ000000000000000000000J`.

The Rust test suite regenerates each compact token in memory with injected
deterministic sources and compares it byte-for-byte with these files. It also
compares each decoded payload semantically with its JSON fixture.
