// Prints a private RSA JWK (RS256 signing key) for a private_key_jwt
// conformance client: the suite signs client assertions with it, and
// op-setup.sh registers its public part in Stargate.
//
// Usage: node client-jwk.mjs <kid>
import crypto from "node:crypto";

const kid = process.argv[2] ?? crypto.randomUUID();
const { privateKey } = crypto.generateKeyPairSync("rsa", { modulusLength: 2048 });
const jwk = { ...privateKey.export({ format: "jwk" }), kid, alg: "RS256", use: "sig" };
console.log(JSON.stringify(jwk));
