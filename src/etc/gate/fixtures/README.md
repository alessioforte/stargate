# Gateway transport test fixtures

These certificates and private keys are public test material, never production
credentials. The test CA signs separate server/client RSA identities; the server
certificate covers `localhost` and `127.0.0.1`. Certificates were generated with
OpenSSL for a 20-year lifetime. The CA private key is not retained.

Tests use the server private key as a valid but mismatched client key, and create
missing, empty, malformed, invalid-DER, and duplicate-key files in temporary
directories. Runtime parsing does not depend on the OpenSSL executable.
