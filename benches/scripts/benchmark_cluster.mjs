#!/usr/bin/env node
/** External open-loop workload driver. Read credentials/configuration from stdin.
 * Uses Node.js and the ws package for WebSockets. Never prints tokens or bodies.
 */
import https from "node:https";
import { readFileSync } from "node:fs";
import { randomBytes, createHash } from "node:crypto";
import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";
let config;
try { config = JSON.parse(readFileSync(0, "utf8")); }
catch { console.error("Invalid benchmark configuration JSON"); process.exit(2); }
const ca = config.caFile ? readFileSync(config.caFile) : undefined;
const agents = new Map();
const duration = config.durationSeconds ?? 15;
const threshold = config.latencyThresholdMs ?? 1000;
const quantile = (sorted, q) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))] ?? 0;
async function request(origin, path, { method = "GET", json, form, token = config.token } = {}) {
  const url = new URL(path, origin);
  const body = json ? JSON.stringify(json) : form ? new URLSearchParams(form).toString() : undefined;
  const headers = { host: url.host };
  if (token) headers.authorization = `Bearer ${token}`;
  if (body) { headers["content-type"] = json ? "application/json" : "application/x-www-form-urlencoded"; headers["content-length"] = Buffer.byteLength(body); }
  if (!agents.has(origin)) agents.set(origin, new https.Agent({ keepAlive: true, maxSockets: config.maxInFlight ?? 512, ca }));
  return await new Promise((resolve, reject) => {
    const req = https.request({ hostname: config.targetIp ?? url.hostname, port: url.port || 443, servername: url.hostname, path: url.pathname + url.search, method, headers, agent: agents.get(origin), timeout: 10000 }, response => {
      let text = "";
      response.on("data", chunk => { text += chunk; });
      response.on("end", () => {
        let data; try { data = JSON.parse(text); } catch { data = {}; }
        if (response.statusCode >= 400) return reject(new Error(`${response.statusCode}:${data.code ?? data.error ?? "http_error"}`));
        resolve({ status: response.statusCode, headers: response.headers, data });
      });
      response.on("error", () => reject(new Error("transport")));
    });
    req.on("error", () => reject(new Error("transport")));
    req.on("timeout", () => req.destroy());
    req.end(body);
  });
}
async function operation() {
  switch (config.scenario) {
    case "gateway": return request(config.apiOrigin, config.gatewayPath ?? "/benchmark/hot");
    case "policies": return request(config.apiOrigin, config.policyPath ?? "/benchmark/policies");
    case "login": return request(config.identityOrigin, "/account/login", { method: "POST", json: { username: config.username, password: config.password }, token: null });
    case "discovery":
      await request(config.identityOrigin, "/.well-known/openid-configuration");
      return request(config.identityOrigin, "/.well-known/jwks.json");
    case "oauth": {
      const verifier = randomBytes(32).toString("base64url");
      const challenge = createHash("sha256").update(verifier).digest("base64url");
      const query = new URLSearchParams({ client_id: config.oauthClientId, redirect_uri: config.redirectUri, response_type: "code", scope: "openid offline_access", code_challenge: challenge, code_challenge_method: "S256" });
      const authorize = await request(config.identityOrigin, `/oauth/authorize?${query}`);
      const redirect = new URL(authorize.headers.location, config.identityOrigin);
      const code = redirect.searchParams.get("code");
      if (!code) throw new Error(`oauth:${redirect.searchParams.get("error") ?? "interaction_required"}`);
      const token = await request(config.identityOrigin, "/oauth/token", { method: "POST", form: { grant_type: "authorization_code", client_id: config.oauthClientId, redirect_uri: config.redirectUri, code, code_verifier: verifier }, token: null });
      if (!token.data.refresh_token) throw new Error("oauth:refresh_token_missing");
      return request(config.identityOrigin, "/oauth/token", { method: "POST", form: { grant_type: "refresh_token", client_id: config.oauthClientId, refresh_token: token.data.refresh_token }, token: null });
    }
    default: throw new Error("unknown_scenario");
  }
}
function summary(rate, times, errors, elapsed, extra = {}) {
  times.sort((a, b) => a - b);
  return { scenario: config.scenario, offered: rate, durationSeconds: elapsed / 1000, completed: times.length, completedPerSecond: times.length / (elapsed / 1000), p50Ms: quantile(times, .50), p95Ms: quantile(times, .95), p99Ms: quantile(times, .99), errors, ...extra };
}
async function httpStage(rate) {
  const started = performance.now(), times = [], errors = {}, pending = new Set();
  let scheduleLagMs = 0;
  const count = Math.ceil(rate * duration);
  for (let i = 0; i < count; i++) {
    const due = started + i * 1000 / rate;
    const wait = due - performance.now();
    if (wait > 0) await sleep(wait);
    scheduleLagMs = Math.max(scheduleLagMs, performance.now() - due);
    if (pending.size >= (config.maxInFlight ?? 512)) { errors["generator.capacity"] = (errors["generator.capacity"] ?? 0) + 1; continue; }
    const begin = performance.now();
    const job = operation().catch(error => { const code = /^[0-9]{3}:[a-zA-Z0-9_.-]+$|^oauth:[a-zA-Z0-9_.-]+$|^transport$/.test(error.message) ? error.message : "generator.error"; errors[code] = (errors[code] ?? 0) + 1; }).finally(() => { times.push(performance.now() - begin); pending.delete(job); });
    pending.add(job);
  }
  await Promise.all(pending);
  return summary(rate, times, errors, performance.now() - started, { maxScheduleLagMs: scheduleLagMs });
}
async function websocketStage(count) {
  const { default: WebSocket } = await import("ws");
  const sockets = [], times = [], errors = {};
  const url = new URL(config.websocketPath ?? "/benchmark/socket", config.apiOrigin.replace("https:", "wss:"));
  const started = performance.now();
  const fail = code => { errors[code] = (errors[code] ?? 0) + 1; };
  try {
    await Promise.all(Array.from({ length: count }, async () => {
      const socket = new WebSocket(url, { ca, headers: { authorization: `Bearer ${config.token}` }, lookup: config.targetIp ? (_name, options, cb) => options.all ? cb(null, [{ address: config.targetIp, family: 4 }]) : cb(null, config.targetIp, 4) : undefined, handshakeTimeout: 10000 });
      sockets.push(socket);
      socket.on("error", () => fail("websocket.transport"));
      await new Promise(resolve => { socket.once("open", resolve); socket.once("error", resolve); socket.once("unexpected-response", (_req, res) => { fail(`${res.statusCode}:websocket.handshake`); socket.terminate(); resolve(); }); });
    }));
    for (let tick = 0; tick < duration; tick++) {
      const due = performance.now();
      await Promise.all(sockets.map(socket => new Promise(resolve => {
        if (socket.readyState !== WebSocket.OPEN) { fail("websocket.closed"); resolve(); return; }
        const sent = performance.now();
        const timeout = setTimeout(() => { socket.off("message", receive); fail("websocket.timeout"); resolve(); }, 5000);
        const receive = () => { clearTimeout(timeout); times.push(performance.now() - sent); resolve(); };
        socket.once("message", receive); socket.send(`tick-${tick}`);
      })));
      await sleep(Math.max(0, 1000 - (performance.now() - due)));
    }
    return summary(count, times, errors, performance.now() - started, { connections: count, messagesPerConnectionPerSecond: 1 });
  } finally {
    await Promise.all(sockets.map(socket => new Promise(resolve => { const timeout = setTimeout(() => { socket.terminate(); resolve(); }, 1500); socket.once("close", () => { clearTimeout(timeout); resolve(); }); socket.close(1000); })));
  }
}
try {
  for (const rate of config.rates ?? [5, 10, 20, 40, 80]) {
    const result = config.scenario === "websocket" ? await websocketStage(rate) : await httpStage(rate);
    console.log(JSON.stringify(result));
    if (Object.keys(result.errors).length || result.p95Ms > threshold || (result.maxScheduleLagMs ?? 0) > 1000) break;
  }
} finally {
  for (const agent of agents.values()) agent.destroy();
}
