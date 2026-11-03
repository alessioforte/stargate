# `ctx` Golden Fixtures

These files freeze the language-neutral version 1 interoperability vectors.

- `*.jwt` contains one compact RS256 token on a single line.
- `*.claims.json` contains the decoded payload for that token.
- `public.pem` and `public.jwk.json` are equivalent verification keys.
- `private.pem` and `rotation-next-private.pem` are test-only fixture material
  and must never be used outside tests. The latter is used only by the
  Stargate key-rotation drill.
- `private.pkcs1.pem` and `public.pkcs1.pem` hold the same key pair in PKCS#1
  (`RSA PRIVATE KEY`, `RSA PUBLIC KEY`); they must sign and verify the same
  tokens.
- `weak-1024-private.pem` is a test-only 1,024-bit key that must be rejected as
  too small.

All vectors use:

- issuer `https://auth.example.com/internal-context`;
- audience `urn:stargate:service:orders`;
- key id `stargate-internal-test`;
- issue time `1784474100` and expiry `1784474130`; and
- dispatch id `01JZ000000000000000000000J`.

The Rust test suite regenerates each compact token in memory with injected
deterministic sources and compares it byte-for-byte with these files. It also
compares each decoded payload semantically with its JSON fixture.
