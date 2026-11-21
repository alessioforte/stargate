#!/usr/bin/env python3
"""Redis failure bench: run a cluster-profile Stargate under Redis failures.

Build first: cargo build --features cluster
Run: python3 benches/scripts/redis_failures.py \
       --redis-image redis:7.4-alpine --postgres-image postgres:17-alpine
Requires Docker. PostgreSQL and Redis containers are disposable, bind to
localhost only and are removed on exit. Only synthetic data is used; the
generated credentials are never printed.
"""

import argparse
import base64
import hashlib
import http.server
import json
import os
import secrets
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from urllib.parse import parse_qs, urlencode, urlsplit
from pathlib import Path

from redis_fixtures import write_transactional_templates

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/debug/stargate"

INGRESS_MS = 250
SECURITY_MS = 1000
# On-wire measurements include HTTP processing and timer/CI scheduling jitter.
# The Rust budget test checks the exact timer with Tokio's clock.
HTTP_MARGIN_MS = 40

CONFIG = """schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params: {max_burst: 10000, replenish_1_per: 1ms}
  ingress:
    strategy: gcra
    params: {max_burst: 10000, replenish_1_per: 1ms}
  bench:
    strategy: gcra
    params: {max_burst: 10000, replenish_1_per: 1h}
  bench_quota:
    strategy: quota_tracker
    params: {limit: 10000, period: day}
  bench_deny:
    strategy: gcra
    params: {max_burst: 10000, replenish_1_per: 1ms}
http:
  services:
    ok: {kind: direct_response, status: 200}
  policies:
    bench: {kind: rate_limit, limit: bench}
    bench_deny: {kind: rate_limit, limit: bench_deny, on_store_error: deny}
    bench_quota: {kind: quota, limit: bench_quota}
    auth: {kind: auth, strategies: [jwt, api_key, oauth], audience: bench}
  routers:
    bench:
      match: {path: {exact: /bench/public}}
      service: ok
      policies: [bench, bench_quota]
      quota_cost: 1
    bench_deny:
      match: {path: {exact: /bench/deny}}
      service: ok
      policies: [bench_deny]
    auth:
      match: {path: {exact: /bench/auth}}
      service: ok
      policies: [auth]
"""

# A localhost TCP relay. While the process is stopped (SIGSTOP) the established
# sockets stay open but no byte crosses: a network partition for Stargate.
RELAY = """
import socket
import sys
import threading


def pump(source, target):
    try:
        while data := source.recv(65536):
            target.sendall(data)
    except OSError:
        pass
    finally:
        try:
            target.shutdown(socket.SHUT_WR)
        except OSError:
            pass


listen_port, target_port = int(sys.argv[1]), int(sys.argv[2])
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", listen_port))
server.listen(128)
while True:
    client, _ = server.accept()
    try:
        upstream = socket.create_connection(("127.0.0.1", target_port))
    except OSError:
        client.close()
        continue
    threading.Thread(target=pump, args=(client, upstream), daemon=True).start()
    threading.Thread(target=pump, args=(upstream, client), daemon=True).start()
"""

ENDPOINTS = {
    "public": ("/bench/public", 3),
    "deny": ("/bench/deny", 3),
    "profile": ("/account/profile", 2),
    "jwt": ("/bench/auth", 2),
    "api_key": ("/bench/auth", 2),
    "oauth": ("/bench/auth", 2),
    "userinfo": ("/oauth/userinfo", 2),
}


def run(args, **kwargs):
    kwargs.setdefault("capture_output", True)
    kwargs.setdefault("text", True)
    return subprocess.run(args, **kwargs)


def docker(*args, **kwargs):
    return run(["docker", *args], **kwargs)


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


HTTP = urllib.request.build_opener(NoRedirect())


def request(url, method="GET", body=None, token=None, api_key=None, timeout=5,
            form=None, client_ip=None):
    headers = {"Content-Type": "application/json"}
    if client_ip:
        headers["X-Forwarded-For"] = client_ip
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if api_key:
        headers["x-api-key"] = api_key
    payload = json.dumps(body).encode() if body is not None else None
    if form is not None:
        headers["Content-Type"] = "application/x-www-form-urlencoded"
        payload = urlencode(form).encode()
    req = urllib.request.Request(url, data=payload, method=method, headers=headers)
    try:
        with HTTP.open(req, timeout=timeout) as response:
            raw = response.read()
            return (
                response.status,
                {key.lower(): value for key, value in response.headers.items()},
                json.loads(raw) if raw else None,
                None,
            )
    except urllib.error.HTTPError as error:
        raw = error.read()
        try:
            decoded = json.loads(raw) if raw else None
        except ValueError:
            decoded = None
        return (
            error.code,
            {key.lower(): value for key, value in error.headers.items()},
            decoded,
            None,
        )
    except (OSError, TimeoutError) as error:
        return None, {}, None, type(error).__name__


def wait_until(predicate, timeout, message):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (OSError, AssertionError):
            pass
        time.sleep(0.1)
    raise AssertionError(message)


class Containers:
    def __init__(self, prefix):
        self.prefix = prefix
        self.ids = []
        self.networks = []

    def run(self, image, port, options=(), command=(), name=None):
        # A fixed host port survives stop/start: with an ephemeral mapping Docker
        # Desktop assigns a new host port on restart and the server could not
        # reconnect. The race against another process binding it is negligible.
        host_port = free_port()
        result = docker(
            "run",
            "-d",
            "--name",
            name or f"{self.prefix}-{secrets.token_hex(5)}",
            "-p",
            f"127.0.0.1:{host_port}:{port}",
            *options,
            image,
            *command,
            check=False,
        )
        require(result.returncode == 0, f"docker run {image} failed: {result.stderr}")
        cid = result.stdout.strip()
        self.ids.append(cid)
        return cid, host_port

    def remove(self, cid):
        docker("rm", "-f", cid, check=False)
        if cid in self.ids:
            self.ids.remove(cid)

    def network(self):
        name = f"{self.prefix}-{secrets.token_hex(5)}"
        docker("network", "create", name, check=True)
        self.networks.append(name)
        return name

    def cleanup(self):
        for cid in reversed(self.ids):
            docker("rm", "-f", cid, check=False)
        self.ids.clear()
        for name in self.networks:
            docker("network", "rm", name, check=False)
        self.networks.clear()


def redis_ready(cid):
    result = docker("exec", cid, "redis-cli", "PING", check=False)
    return result.returncode == 0 and result.stdout.strip() == "PONG"


def redis_command(cid, *command, input=None):
    return docker("exec", "-i", cid, "redis-cli", *command, input=input, check=False)


def wait_redis(cid, timeout=30):
    wait_until(lambda: redis_ready(cid), timeout, "Redis did not become ready")


def wait_redis_writable(cid):
    wait_until(
        lambda: redis_command(cid, "SET", "bench:readiness", "1", "EX", "10").stdout.strip() == "OK",
        30,
        "Redis did not accept writes",
    )
    return time.monotonic()


def postgres_ready(cid):
    # The image starts a temporary Unix-socket server during init; only the
    # final TCP listener can serve the mapped port.
    result = docker(
        "exec",
        cid,
        "pg_isready",
        "-h",
        "127.0.0.1",
        "-U",
        "postgres",
        "-t",
        "1",
        check=False,
    )
    return result.returncode == 0


class Server:
    def __init__(self, directory, env, base, username, password):
        self.directory = directory
        self.env = env
        self.base = base
        self.username = username
        self.password = password
        self.api_key = None
        self.log_path = directory / "server.log"
        self.start()

    def start(self):
        self.log_handle = self.log_path.open("a")
        self.proc = subprocess.Popen(
            [str(BINARY)], env=self.env, cwd=str(self.directory),
            stdout=self.log_handle, stderr=subprocess.STDOUT,
        )

    def url(self, path):
        return self.base + path

    def wait_ready(self, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.proc.poll() is not None:
                raise AssertionError(
                    "server exited before readiness:\n" + self.redacted_log()
                )
            status, _, _, _ = request(self.url("/readyz"), timeout=2)
            if status == 200:
                return
            time.sleep(0.1)
        raise AssertionError("server readiness timeout:\n" + self.redacted_log())

    def login(self):
        status, _, body, _ = request(
            self.url("/account/login"),
            "POST",
            {"username": self.username, "password": self.password},
        )
        require(status == 200, f"login status {status}")
        return body["accessToken"], body["refreshToken"]

    def create_admin_key(self, token):
        status, _, body, _ = request(
            self.url("/admin/admin-keys"),
            "POST",
            {"label": "redis-failure-bench", "permissions": ["health:read"]},
            token=token,
        )
        require(status == 201, f"admin key status {status}")
        self.api_key = body["apiKey"]

    def gateway_credentials(self, native):
        user_id = claims(native)["sub_id"]
        status, _, key, _ = request(
            self.url("/admin/api-keys"), "POST",
            {"userId": user_id, "label": "redis-failure-bench"}, token=native,
        )
        require(status == 201, f"gateway key creation status {status}")
        client_id = "redis_failure_bench"
        redirect = "http://127.0.0.1:32123/callback"
        status, _, _, _ = request(
            self.url("/admin/oauth/clients"), "POST",
            {
                "clientId": client_id, "name": "Redis failure bench",
                "tokenEndpointAuthMethod": "none",
                "grantTypes": ["authorization_code", "refresh_token"],
                "responseTypes": ["code"], "redirectUris": [redirect],
                "scopes": ["openid", "offline_access"], "audiences": ["bench"],
                "attrs": {"first_party": True},
            },
            token=native,
        )
        require(status == 201, f"OAuth client creation status {status}")
        verifier = secrets.token_urlsafe(48)
        challenge = base64.urlsafe_b64encode(
            hashlib.sha256(verifier.encode()).digest()
        ).decode().rstrip("=")
        query = urlencode({
            "client_id": client_id, "response_type": "code",
            "redirect_uri": redirect, "scope": "openid offline_access",
            "audience": "bench", "code_challenge": challenge,
            "code_challenge_method": "S256",
        })
        status, headers, _, _ = request(self.url(f"/oauth/authorize?{query}"), token=native)
        require(status in (302, 303), f"OAuth authorization status {status}")
        params = parse_qs(urlsplit(headers["location"]).query)
        require("code" in params, f"OAuth authorization did not issue a code: {params.get('error')}")
        status, _, tokens, _ = request(
            self.url("/oauth/token"), "POST",
            form={
                "grant_type": "authorization_code", "client_id": client_id,
                "code": params["code"][0], "redirect_uri": redirect,
                "code_verifier": verifier,
            },
        )
        require(status == 200, f"OAuth code redemption status {status}")
        return key["apiKey"], tokens["access_token"], {
            "grant_type": "refresh_token", "client_id": client_id,
            "refresh_token": tokens["refresh_token"],
        }

    def health(self):
        status, _, body, error = request(
            self.url("/admin/health"), api_key=self.api_key, timeout=5
        )
        require(
            status == 200,
            f"admin health status {status} ({error or (body or {}).get('code')})",
        )
        return body

    def redacted_log(self):
        text = (
            self.log_path.read_text(errors="replace") if self.log_path.exists() else ""
        )
        for secret in (self.password, self.api_key):
            if secret:
                text = text.replace(secret, "[redacted]")
        return text

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.log_handle.close()


def minimal_env():
    keep = ("PATH", "HOME", "TMPDIR", "RUSTUP_HOME", "CARGO_HOME")
    return {key: os.environ[key] for key in keep if key in os.environ}


def start_server(ctx, name, redis_url, config=CONFIG, extra_env=None):
    directory = Path(tempfile.mkdtemp(prefix=f"redis-failure-{name}-"))
    write_transactional_templates(directory)
    (directory / "config.yaml").write_text(config)
    database = "bench_" + name.replace("-", "_")
    username = database
    password = secrets.token_urlsafe(30) + "Aa1!"
    port = free_port()
    env = minimal_env()
    env.update(
        CONFIG_PATH=str(directory),
        POSTGRES_ENDPOINT=f"127.0.0.1:{ctx.postgres_port}",
        POSTGRES_USERNAME="postgres",
        POSTGRES_PASSWORD="",
        POSTGRES_DATABASE=database,
        POSTGRES_CREATE_DATABASE="true",
        POSTGRES_SSLMODE="disable",
        REDIS_URL=redis_url,
        MFA_MODE="off",
        JWT_ALGORITHM="HS256",
        JWT_SECRET=secrets.token_urlsafe(48),
        JWT_ISSUER=f"http://127.0.0.1:{port}",
        PORT=str(port),
        LOG_FORMAT="json",
        RUST_LOG="warn",
        OTEL_ENABLED="false",
        SERVER_DRAIN_DELAY_SECS="0",
        TRUSTED_PROXIES="127.0.0.1",
    )
    env.update(extra_env or {})
    require(BINARY.exists(), f"{BINARY} is missing; run cargo build --features cluster")
    result = run(
        [
            str(BINARY),
            "admin",
            "bootstrap",
            "--email",
            f"{username}@example.com",
            "--password-stdin",
            "--name",
            "Redis failure bench",
            "--nickname",
            username,
        ],
        input=password + "\n",
        env=env,
        cwd=str(directory),
        check=False,
    )
    require(result.returncode == 0, "admin bootstrap failed: " + result.stderr)
    server = Server(directory, env, f"http://127.0.0.1:{port}", username, password)
    ctx.servers.append(server)
    return server


def start_cluster(ctx, name, redis_options):
    redis, redis_port = ctx.containers.run(
        ctx.args.redis_image, 6379, command=redis_options
    )
    wait_redis(redis)
    server = start_server(ctx, name, f"redis://127.0.0.1:{redis_port}")
    server.wait_ready()
    token, refresh = server.login()
    server.create_admin_key(token)
    return redis, server, token, refresh


class Load:
    def __init__(self, base, token, credentials=None, clients=2, interval=0.1):
        self.base = base
        self.token = token
        self.credentials = credentials
        self.endpoints = ("public", "deny", "profile")
        if credentials:
            self.endpoints += ("jwt", "api_key", "oauth", "userinfo")
        self.clients = clients
        self.interval = interval
        self.samples = []
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.threads = []

    def start(self):
        # One pool per endpoint: a session read blocking on the store timeout
        # must not pause the availability probes.
        for name in self.endpoints:
            for _ in range(self.clients):
                thread = threading.Thread(
                    target=self._worker, args=(name,), daemon=True
                )
                thread.start()
                self.threads.append(thread)

    def stop(self):
        self.stopped.set()
        for thread in self.threads:
            thread.join(timeout=15)

    def set_token(self, token):
        self.token = token

    def snapshot(self):
        with self.lock:
            return list(self.samples)

    def _worker(self, name):
        while not self.stopped.is_set():
            self._sample(name)
            if self.stopped.wait(self.interval):
                return

    def _sample(self, name):
        path, timeout = ENDPOINTS[name]
        started = time.monotonic()
        token = self.token if name in ("profile", "jwt") else None
        api_key = None
        if name == "api_key":
            api_key = self.credentials[0]
        if name in ("oauth", "userinfo"):
            token = self.credentials[1]
        status, headers, body, error = request(
            self.base + path, token=token, api_key=api_key, timeout=timeout
        )
        sample = {
            "name": name,
            "at": started,
            "done": time.monotonic(),
            "status": status,
            "allowance": "x-ratelimit-limit" in headers,
            "quota": "x-quota-remaining" in headers,
            "retry_after": headers.get("retry-after"),
            "code": body.get("code") if isinstance(body, dict) else None,
            "error": error,
            "latency_ms": round((time.monotonic() - started) * 1000),
        }
        with self.lock:
            self.samples.append(sample)


def between(samples, start, end):
    return [sample for sample in samples if start <= sample["at"] <= end]


def expect(samples, predicate, label):
    bad = [sample for sample in samples if not predicate(sample)]
    if bad:
        raise AssertionError(
            f"{label}: {len(bad)} unexpected of {len(samples)}: {bad[0]}"
        )
    return 0


def healthy_sample(sample):
    return (
        sample["status"] == 200
        and (sample["name"] not in ("public", "deny") or sample["allowance"])
        and (sample["name"] != "public" or sample["quota"])
    )


def unavailable_sample(sample):
    if sample["name"] == "public":
        return (
            sample["status"] == 200 and not sample["allowance"] and not sample["quota"]
            and sample["latency_ms"] <= INGRESS_MS + HTTP_MARGIN_MS
        )
    if sample["name"] == "deny":
        return (
            sample["status"] == 503
            and sample["code"] == "gateway.limiter_unavailable"
            and sample["retry_after"] == "10"
            and sample["latency_ms"] <= INGRESS_MS + HTTP_MARGIN_MS
        )
    return (
        sample["status"] == 503 and sample["retry_after"] == "10"
        and sample["latency_ms"] <= SECURITY_MS + HTTP_MARGIN_MS
    )


def memory_during(sample):
    # Writes fail closed, reads keep working: only session writes are rejected.
    if sample["name"] == "profile":
        return sample["status"] == 200
    return unavailable_sample(sample)


def require_samples(samples, label):
    require(samples, f"{label}: no load samples collected")


def settled(server, token):
    """The documented healthy state: limits enforced, sessions usable."""

    def check():
        status, headers, _, _ = request(server.url("/bench/public"), timeout=3)
        if status != 200 or "x-ratelimit-limit" not in headers:
            return False
        status, _, _, _ = request(
            server.url("/account/profile"), token=token, timeout=20
        )
        if status != 200:
            return False
        status, _, _, _ = request(
            server.url("/account/login"),
            "POST",
            {"username": server.username, "password": server.password},
        )
        if status != 200:
            return False
        health = server.health()
        return (
            health["status"] == "healthy"
            and health["redis"]["status"] == "healthy"
            and not health["limits"]["storeDegraded"]
        )

    return check


def windows(
    samples,
    baseline_start,
    fault_start,
    fault_end,
    recovered,
    start_guard=0.3,
    end_guard=0.3,
    before_end=None,
):
    before = between(samples, baseline_start, (before_end or fault_start) - start_guard)
    disturbed = between(samples, fault_start + start_guard, fault_end - end_guard)
    during = between(samples, fault_start + start_guard, fault_end - 1.1)
    # Keep overlong responses from the middle of the outage in the assertions.
    # They must fail the budget even if they only complete after recovery.
    after = between(samples, recovered + start_guard, time.monotonic())
    return before, during, disturbed, after


def recovery_by_endpoint(load, since, endpoints=None):
    samples = load.snapshot()
    times = {}
    for endpoint in endpoints or load.endpoints:
        healthy = [
            sample["done"] for sample in samples
            if sample["name"] == endpoint and sample["at"] >= since and healthy_sample(sample)
        ]
        require(healthy, f"{endpoint}: no successful sample after Redis accepted writes")
        times[endpoint] = round((min(healthy) - since) * 1000)
    traffic = [times[name] for name in ("public", "deny") if name in times]
    if traffic:
        require(min(traffic) <= 1000, f"first traffic check recovered too late: {times}")
    require(max(times.values()) <= 5000, f"endpoint recovery exceeded 5s: {times}")
    return times


def count_errors(samples):
    """Failed responses observed while the fault was active.

    `unexpected` checks the phase expectation and stops the run, so it is always
    zero in a passing report. This counts the responses that failed during the
    disturbance: no status (client timeout) or `>= 400`.
    """
    errors = {"total": 0} | dict.fromkeys(ENDPOINTS, 0)
    for sample in samples:
        if sample["status"] is None or sample["status"] >= 400:
            errors["total"] += 1
            errors[sample["name"]] += 1
    return errors


def finish(
    name,
    load,
    before,
    during,
    disturbed,
    after,
    during_predicate,
    disturbance,
    recovery,
    final,
):
    unexpected = (
        expect(before, healthy_sample, f"{name} before")
        + expect(during, during_predicate, f"{name} during")
        + expect(after, healthy_sample, f"{name} after")
    )
    return {
        "name": name,
        "requests": len(load.snapshot()),
        "unexpected": unexpected,
        "errors": count_errors(disturbed),
        "disturbance_seconds": round(disturbance, 2),
        "recovery_seconds": round(recovery, 2),
        "final": final,
    }


def scenario_restart(ctx):
    redis, server, token, _ = start_cluster(ctx, "restart", ["--appendonly", "yes"])
    load = Load(server.base, token)
    load.start()
    time.sleep(0.6)
    baseline_start = time.monotonic()
    time.sleep(0.6)

    apply_start = time.monotonic()
    docker("stop", redis)
    require(not redis_ready(redis), "Redis still answers after stop")
    # The outage starts once the graceful stop has completed.
    fault_start = time.monotonic()

    status, headers, _, _ = request(server.url("/bench/public"))
    require(status == 200, f"public route during stop: {status}")
    require("x-ratelimit-limit" not in headers, "fail-open emitted allowance headers")
    status, _, body, _ = request(server.url("/bench/deny"))
    require(
        status == 503 and body["code"] == "gateway.limiter_unavailable",
        f"deny route during stop: {status} {body}",
    )
    status, _, _, _ = request(server.url("/account/profile"), token=token, timeout=20)
    require(
        status is not None and status >= 500,
        f"profile did not fail closed while the store was down: {status}",
    )
    health = server.health()
    require(health["redis"]["status"] == "unhealthy", "health missed the Redis outage")
    require(
        health["limits"]["storeDegraded"], "health missed the degraded limit checks"
    )
    require(request(server.url("/readyz"))[0] == 200, "readiness followed Redis")

    time.sleep(1.6)
    docker("start", redis)
    fault_end = time.monotonic()
    redis_writable = wait_redis_writable(redis)
    wait_until(settled(server, token), 5, "limits and sessions did not recover")
    recovered = time.monotonic()
    require(server.proc.poll() is None, "Stargate restarted during the failure")
    require(
        request(server.url("/account/profile"), token=token)[0] == 200,
        "session did not survive a Redis restart",
    )
    time.sleep(0.4)
    load.stop()
    server.stop()

    before, during, disturbed, after = windows(
        load.snapshot(),
        baseline_start,
        fault_start,
        fault_end,
        recovered,
        0.3,
        0.3,
        apply_start,
    )
    for window, label in ((before, "before"), (during, "during"), (after, "after")):
        require_samples(window, label)
    result = finish(
        "restart",
        load,
        before,
        during,
        disturbed,
        after,
        unavailable_sample,
        fault_end - fault_start,
        recovered - fault_end,
        "healthy",
    )

    result["recovery_by_endpoint_ms"] = recovery_by_endpoint(load, redis_writable)
    return result


def scenario_empty(ctx):
    redis, server, token, refresh = start_cluster(
        ctx, "empty", ["--save", "", "--appendonly", "no"]
    )
    sid = claims(token)["sid"]
    session_key = f"stargate:v1:{{s:{sid}}}:subject"
    require(
        redis_command(redis, "EXISTS", session_key).stdout.strip() == "1",
        "session is not in the store before the restart",
    )
    load = Load(server.base, token)
    load.start()
    time.sleep(0.6)
    baseline_start = time.monotonic()
    time.sleep(0.6)

    apply_start = time.monotonic()
    docker("stop", redis)
    require(not redis_ready(redis), "Redis still answers after stop")
    # The outage starts once the graceful stop has completed.
    fault_start = time.monotonic()
    status, _, _, _ = request(server.url("/account/profile"), token=token, timeout=20)
    require(
        status is not None and status >= 500,
        f"profile did not fail closed while the store was down: {status}",
    )

    time.sleep(1.6)
    docker("start", redis)
    fault_end = time.monotonic()
    redis_writable = wait_redis_writable(redis)
    require(
        redis_command(redis, "EXISTS", session_key).stdout.strip() == "0",
        "the restarted Redis still holds the session",
    )
    wait_until(
        lambda: request(server.url("/account/profile"), token=token)[0] == 401,
        5,
        "stale profile did not sign out after empty restart",
    )
    status, _, body, _ = request(
        server.url("/account/refresh-token"), "PUT", {"refresh_token": refresh}
    )
    require(status == 401, f"stale refresh token: {status} {body and body.get('code')}")

    new_token, _ = server.login()
    load.set_token(new_token)
    wait_until(settled(server, new_token), 30, "limits and sign-in did not recover")
    recovered = time.monotonic()
    require(
        request(server.url("/account/profile"), token=new_token)[0] == 200,
        "a fresh sign-in is not usable after an empty Redis restart",
    )
    time.sleep(0.4)
    load.stop()
    server.stop()

    # The post-restart window uses the new session: the old one is signed out,
    # the service is not permanently broken.
    before, during, disturbed, after = windows(
        load.snapshot(),
        baseline_start,
        fault_start,
        fault_end,
        recovered,
        0.3,
        0.3,
        apply_start,
    )
    for window, label in ((before, "before"), (during, "during"), (after, "after")):
        require_samples(window, label)
    result = finish(
        "empty",
        load,
        before,
        during,
        disturbed,
        after,
        unavailable_sample,
        fault_end - fault_start,
        recovered - fault_end,
        "signed-out then healthy",
    )

    result["recovery_by_endpoint_ms"] = recovery_by_endpoint(load, redis_writable, ("public", "deny"))
    return result


class Proxy:
    def __init__(self, target):
        self.port = free_port()
        self.proc = None
        self.start(target)

    def start(self, target):
        self.proc = subprocess.Popen(
            [sys.executable, "-c", RELAY, str(self.port), str(target)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        wait_until(lambda: _port_open(self.port), 10, "the Redis relay did not start")

    def close(self):
        if self.proc.poll() is None:
            # SIGTERM cannot be handled with the relay stopped.
            os.kill(self.proc.pid, signal.SIGCONT)
            self.proc.terminate()
            self.proc.wait(timeout=5)

    @property
    def url(self):
        return f"redis://127.0.0.1:{self.port}"


class HeldUpstream:
    def __init__(self):
        self.started = threading.Event()
        self.release = threading.Event()
        held = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                held.started.set()
                held.release.wait(timeout=10)
                self.send_response(200)
                self.send_header("Content-Length", "0")
                self.end_headers()

            def log_message(self, *_args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def config(self):
        port = self.server.server_port
        return CONFIG.replace(
            "  services:\n",
            f"  upstreams:\n    held: {{targets: [{{url: 'http://127.0.0.1:{port}'}}]}}\n  services:\n"
            "    held: {kind: load_balancer, upstream: held}\n",
            1,
        ) + "    held:\n      match: {path: {exact: /bench/held}}\n      service: held\n"

    def close(self):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


def timed_unavailable(server, path, expected_code, budget, **kwargs):
    started = time.monotonic()
    status, headers, body, _ = request(server.url(path), **kwargs)
    elapsed = round((time.monotonic() - started) * 1000)
    require(
        status == 503 and headers.get("retry-after") == "10"
        and (body or {}).get("code", (body or {}).get("error")) == expected_code,
        f"{path}: wanted 503 {expected_code} with Retry-After, got {status} after {elapsed}ms",
    )
    require(elapsed <= budget + HTTP_MARGIN_MS, f"{path}: {elapsed}ms exceeds {budget}ms budget")


def scenario_partition(ctx):
    redis, redis_port = ctx.containers.run(ctx.args.redis_image, 6379)
    wait_redis(redis)
    proxy = Proxy(redis_port)
    upstream = HeldUpstream()
    load = None
    try:
        server = start_server(ctx, "partition", proxy.url, config=upstream.config())
        server.wait_ready()
        token, _ = server.login()
        server.create_admin_key(token)
        credentials = server.gateway_credentials(token)

        deny = start_server(
            ctx, "partition-deny", proxy.url,
            config=CONFIG.replace("  timeout: 250ms", "  timeout: 250ms\n  on_store_error: deny"),
        )
        deny.wait_ready()
        require(request(deny.url("/account/features"))[0] == 200, "IAM deny fixture failed healthy")
        require(request(deny.url("/bench/public"))[0] == 200, "gateway deny fixture failed healthy")

        load = Load(server.base, token, credentials=credentials)
        load.start()
        time.sleep(0.6)
        baseline_start = time.monotonic()
        time.sleep(0.6)

        in_flight = []
        held = threading.Thread(
            target=lambda: in_flight.append(request(server.url("/bench/held"), timeout=8)),
            daemon=True,
        )
        held.start()
        require(upstream.started.wait(timeout=3), "request never reached upstream")

        os.kill(proxy.proc.pid, signal.SIGSTOP)
        fault_start = time.monotonic()
        time.sleep(0.5)
        upstream.release.set()
        held.join(timeout=3)
        require(in_flight and in_flight[0][0] == 200, "partition interrupted a dispatched request")

        timed_unavailable(deny, "/account/features", "iam.limiter_unavailable", INGRESS_MS)
        timed_unavailable(deny, "/bench/public", "gateway.ingress_unavailable", INGRESS_MS)
        timed_unavailable(
            server, "/oauth/token", "server_error", SECURITY_MS,
            method="POST", form=credentials[2],
        )
        require(request(deny.url("/readyz"))[0] == 200, "deny readiness followed Redis")
        require(request(server.url("/readyz"))[0] == 200, "allow readiness followed Redis")
        health = server.health()
        require(health["redis"]["status"] == "unhealthy", "health missed the partition")
        require(health["status"] == "unhealthy", "overall health missed the partition")
        require(health["limits"]["storeDegraded"], "health missed the degraded limit checks")
        for point in ("gateway_ingress", "rate_limit", "quota", "iam_ingress", "admin_principal"):
            require(health["limits"]["points"][point], f"{point} missed the shared budget exhaustion")

        isolated_ip = "203.0.113.27"
        status, headers, _, _ = request(server.url("/bench/public"), client_ip=isolated_ip)
        require(status == 200 and "x-quota-remaining" not in headers, "partition charged quota")

        os.kill(proxy.proc.pid, signal.SIGCONT)
        fault_end = time.monotonic()
        wait_until(settled(server, token), 5, "limits and sessions did not recover")
        recovered = time.monotonic()
        require(all(s.proc.poll() is None for s in (server, deny)), "Stargate exited during partition")
        deny_recovery = {}
        for name, path, needs_allowance in (
            ("iam_ingress_deny", "/account/features", False),
            ("gateway_ingress_deny", "/bench/public", True),
        ):
            def check():
                status, headers, _, _ = request(deny.url(path))
                return status == 200 and (not needs_allowance or "x-quota-remaining" in headers)
            wait_until(check, 5, f"{name} did not recover")
            deny_recovery[name] = round((time.monotonic() - fault_end) * 1000)
            require(deny_recovery[name] <= 5000, f"{name} recovery exceeded 5s")

        status, headers, _, _ = request(server.url("/bench/public"), client_ip=isolated_ip)
        require(
            status == 200 and headers.get("x-quota-remaining") == "9999"
            and headers.get("x-ratelimit-remaining") == "9999",
            "recovery did not charge the rate and quota buckets exactly once",
        )
        status, _, _, _ = request(server.url("/oauth/token"), "POST", form=credentials[2])
        require(status == 200, f"OAuth refresh did not recover: {status}")
        refresh_recovery = round((time.monotonic() - fault_end) * 1000)
        require(refresh_recovery <= 5000, f"OAuth refresh recovery exceeded 5s: {refresh_recovery}ms")
        time.sleep(0.5)
        load.stop()
    finally:
        if load:
            load.stop()
        proxy.close()
        upstream.close()

    before, during, disturbed, after = windows(
        load.snapshot(), baseline_start, fault_start, fault_end, recovered
    )
    for name in load.endpoints:
        for label, window in (("before", before), ("during", during), ("after", after)):
            require_samples([sample for sample in window if sample["name"] == name], f"{name} {label}")
    result = finish(
        "partition", load, before, during, disturbed, after, unavailable_sample,
        fault_end - fault_start, recovered - fault_end, "healthy",
    )
    result["recovery_by_endpoint_ms"] = recovery_by_endpoint(load, fault_end) | deny_recovery | {
        "oauth_refresh": refresh_recovery
    }
    return result


def scenario_memory_full(ctx):
    redis, server, token, _ = start_cluster(
        ctx,
        "memory",
        [
            "--maxmemory",
            "2mb",
            "--maxmemory-policy",
            "noeviction",
            "--save",
            "",
            "--appendonly",
            "no",
        ],
    )
    load = Load(server.base, token)
    load.start()
    time.sleep(0.6)
    baseline_start = time.monotonic()
    time.sleep(0.6)

    # The baseline ends before the fill: once memory is near the limit, even the
    # limit checks start failing open.
    fill_start = time.monotonic()
    payload = "x" * (128 * 1024)
    for index in range(16):
        redis_command(redis, "-x", "SET", f"filler:{index}", input=payload)
    # Redis rejects a write only once it is already over the limit, so pin the
    # limit below the stored data: every denyoom write is rejected from now on
    # while reads keep working.
    redis_command(redis, "CONFIG", "SET", "maxmemory", "100kb")
    probe = redis_command(redis, "SET", "filler:probe", "1")
    require(
        "OOM" in probe.stdout + probe.stderr,
        "Redis accepted a write at maxmemory with noeviction",
    )
    fault_start = time.monotonic()

    status, headers, _, _ = request(server.url("/bench/public"))
    require(status == 200, f"public route with a full Redis: {status}")
    require("x-ratelimit-limit" not in headers, "fail-open emitted allowance headers")
    status, _, body, _ = request(server.url("/bench/deny"))
    require(
        status == 503 and body["code"] == "gateway.limiter_unavailable",
        f"deny route with a full Redis: {status} {body}",
    )
    status, _, body, _ = request(
        server.url("/account/login"),
        "POST",
        {"username": server.username, "password": server.password},
    )
    require(
        status >= 500,
        f"sign-in reported success while session writes were rejected: {status}",
    )
    status, _, _, _ = request(server.url("/account/profile"), token=token, timeout=20)
    require(status == 200, f"store reads stopped working at maxmemory: {status}")
    health = server.health()
    require(health["redis"]["status"] == "healthy", "PING failed at maxmemory")
    require(health["status"] == "degraded", f"overall health: {health['status']}")
    require(
        health["limits"]["storeDegraded"], "health missed the degraded limit checks"
    )
    require(request(server.url("/readyz"))[0] == 200, "readiness followed Redis")

    time.sleep(1)
    fault_end = time.monotonic()
    keys = redis_command(redis, "--scan", "--pattern", "filler:*").stdout.split()
    if keys:
        redis_command(redis, "DEL", *keys)
    # Restore the configured limit as well: the empty instance still uses more
    # than 100kb of baseline memory.
    redis_command(redis, "CONFIG", "SET", "maxmemory", "2mb")
    wait_until(settled(server, token), 30, "limits and writes did not recover")
    recovered = time.monotonic()
    require(
        request(
            server.url("/account/login"),
            "POST",
            {"username": server.username, "password": server.password},
        )[0]
        == 200,
        "sign-in did not recover after freeing memory",
    )
    time.sleep(0.4)
    load.stop()
    server.stop()

    # The fault begins when Redis starts rejecting writes, after the fill.
    samples = load.snapshot()
    before = between(samples, baseline_start, fill_start - 0.3)
    disturbed = between(samples, fault_start + 0.2, fault_end - 0.3)
    during = [sample for sample in disturbed if sample["done"] <= fault_end - 0.3]
    after = between(samples, recovered + 0.3, time.monotonic())
    for window, label in ((before, "before"), (during, "during"), (after, "after")):
        require_samples(window, label)
    return finish(
        "memory-full",
        load,
        before,
        during,
        disturbed,
        after,
        memory_during,
        fault_end - fault_start,
        recovered - fault_end,
        "healthy",
    )


def scenario_absent(ctx):
    redis, server, token, _ = start_cluster(ctx, "absent", ["--appendonly", "yes"])
    server.stop()
    docker("stop", redis, check=True)
    require(not redis_ready(redis), "Redis was up before the cold start")

    started = time.monotonic()
    server.start()
    server.wait_ready()
    require(server.health()["redis"]["status"] == "unhealthy", "Redis outage not reported")
    require(request(server.url("/account/profile"), token=token)[0] == 503, "session did not fail closed")
    require(request(server.url("/readyz"))[0] == 200, "readiness followed Redis")
    fault_end = time.monotonic()
    docker("start", redis, check=True)
    redis_writable = wait_redis_writable(redis)
    wait_until(settled(server, token), 5, "Stargate did not recover from starting without Redis")
    require(server.proc.poll() is None, "Stargate restarted after Redis came up")
    return scenario_summary("absent-at-startup", fault_end - started, time.monotonic() - redis_writable)


def scenario_incompatible(ctx):
    redis, port = ctx.containers.run(ctx.args.unsupported_redis_image, 6379)
    wait_redis(redis)
    server = start_server(ctx, "incompatible", f"redis://127.0.0.1:{port}")
    server.wait_ready()
    status, headers, _, _ = request(server.url("/bench/public"))
    require(status == 200 and "x-quota-remaining" not in headers, "old Redis was used for state")
    wait_until(
        lambda: "HEXPIRE requires Redis 7.4 or Valkey 9.0" in server.redacted_log(),
        1, "missing Redis capability diagnostic",
    )
    return scenario_summary("incompatible", final="ready, Redis rejected")


def scenario_replicated_logout(ctx):
    network = ctx.containers.network()
    primary_name = f"{network}-primary"
    primary, primary_port = ctx.containers.run(
        ctx.args.redis_image, 6379, ["--network", network],
        ["--repl-diskless-sync-delay", "0"], name=primary_name,
    )
    wait_redis(primary)
    proxy = Proxy(primary_port)
    env = {"REDIS_WAIT_REPLICAS": "1", "REDIS_WAIT_TIMEOUT_MS": "250"}
    try:
        server = start_server(ctx, "replicated", proxy.url, extra_env=env)
        server.wait_ready()
        token, refresh = server.login()
        before, _ = server.login()

        # No replica yet. Test both shortfall policies with live WAIT.
        timed_unavailable(server, "/account/logout", "auth.state_unavailable", 1500,
                          method="DELETE", token=before)
        warn = start_server(
            ctx, "replicated-warn", proxy.url,
            extra_env=env | {"REDIS_WAIT_FAIL_ON_TIMEOUT": "false"},
        )
        warn.wait_ready()
        warning_token, _ = warn.login()
        status, _, _, _ = request(warn.url("/account/logout"), "DELETE", token=warning_token)
        require(status == 200, f"warn-only WAIT returned {status}")
        wait_until(lambda: "Redis write replication timed out" in warn.redacted_log(),
                   1, "missing WAIT warning")

        replica, replica_port = ctx.containers.run(
            ctx.args.redis_image, 6379, ["--network", network],
            ["--replicaof", primary_name, "6379"],
        )
        key = f"stargate:v1:{{s:{claims(token)['sid']}}}:subject"
        wait_until(
            lambda: redis_command(replica, "EXISTS", key).stdout.strip() == "1",
            30,
            "target session was never present on the replica",
        )
        status, _, _, _ = request(server.url("/account/logout"), "DELETE", token=token)
        require(status == 200, f"confirmed logout status {status}")

        # Switch the same endpoint to the replica. Keep the Stargate process and
        # its pool alive so this also exercises reconnect after a promotion.
        docker("kill", primary, check=True)
        require(
            redis_command(replica, "REPLICAOF", "NO", "ONE").stdout.strip() == "OK",
            "promotion failed",
        )
        wait_redis_writable(replica)
        proxy.close()
        proxy.start(replica_port)
        wait_until(lambda: request(server.url("/bench/public"))[1].get("x-quota-remaining"),
                   5, "Stargate did not use the promoted replica")
        require(redis_command(replica, "EXISTS", key).stdout.strip() == "0", "replica resurrected subject")
        require(request(server.url("/account/profile"), token=token)[0] == 401, "logout was lost")
        require(
            request(server.url("/account/refresh-token"), "PUT", {"refresh_token": refresh})[0] == 401,
            "refresh resurrected logged-out session",
        )
        fresh, _ = server.login()
        require(request(server.url("/account/profile"), token=fresh)[0] == 200, "promoted server is not usable")

        # The dedicated WAIT connection reconnects on use after promotion.
        # Exercise it on a separate session before measuring shortfall behavior.
        probe, _ = server.login()

        def wait_connection_recovered():
            status, _, _, _ = request(server.url("/account/logout"), "DELETE", token=probe)
            return status == 503 and request(server.url("/account/profile"), token=probe)[0] == 401

        wait_until(wait_connection_recovered, 5, "WAIT connection did not reach the promoted primary")

        # The promoted primary has no replicas. A shortfall must be reported
        # after every session has been revoked on that primary.
        second, _ = server.login()
        third, _ = server.login()
        sessions = (fresh, second, third)
        for session in sessions:
            require(request(server.url("/account/profile"), token=session)[0] == 200,
                    "session was inactive before logout everywhere")
        timed_unavailable(server, "/account/logout/all", "auth.state_unavailable", 2000,
                          method="POST", token=fresh)
        for session in sessions:
            status, _, _, _ = request(server.url("/account/profile"), token=session)
            require(status == 401, f"expected revoked session after replication shortfall, got {status}")
    finally:
        proxy.close()
    return scenario_summary("replicated-logout", final="logout survived promotion")


def scenario_summary(name, disturbance=0, recovery=0, final="healthy"):
    return {
        "name": name,
        "requests": 0,
        "unexpected": 0,
        "errors": {"total": 0, "public": 0, "deny": 0, "profile": 0},
        "disturbance_seconds": round(disturbance, 2),
        "recovery_seconds": round(recovery, 2),
        "final": final,
    }


def claims(token):
    value = token.split(".")[1]
    return json.loads(base64.urlsafe_b64decode(value + "=" * (-len(value) % 4)))


def _port_open(port):
    with socket.socket() as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


class Context:
    def __init__(self, args):
        self.args = args
        self.containers = Containers(args.container_prefix)
        self.servers = []
        self.postgres_port = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--redis-image", required=True)
    parser.add_argument("--postgres-image", required=True)
    parser.add_argument(
        "--scenarios",
        default="restart,empty,partition,memory-full,absent-at-startup,incompatible,replicated-logout",
        help="comma-separated scenario names, all by default",
    )
    parser.add_argument("--container-prefix", default="stargate-redis-failure")
    parser.add_argument("--unsupported-redis-image", default="redis:7.2-alpine")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    scenarios = {
        "restart": scenario_restart,
        "empty": scenario_empty,
        "partition": scenario_partition,
        "memory-full": scenario_memory_full,
        "absent-at-startup": scenario_absent,
        "incompatible": scenario_incompatible,
        "replicated-logout": scenario_replicated_logout,
    }
    selected = [name.strip() for name in args.scenarios.split(",") if name.strip()]
    unknown = sorted(set(selected) - scenarios.keys())
    require(not unknown, f"unknown scenarios: {', '.join(unknown)}")

    ctx = Context(args)
    results = []
    try:
        cid, port = ctx.containers.run(
            args.postgres_image, 5432, ["-e", "POSTGRES_HOST_AUTH_METHOD=trust"]
        )
        ctx.postgres_port = port
        wait_until(lambda: postgres_ready(cid), 60, "PostgreSQL did not become ready")
        for name in selected:
            results.append(scenarios[name](ctx))
    finally:
        for server in reversed(ctx.servers):
            server.stop()
        ctx.containers.cleanup()

    print(
        f"{'scenario':<20}{'requests':>9}{'unexpected':>12}{'errors':>8}"
        f"{'public':>8}{'deny':>7}{'profile':>9}"
        f"{'disturbance_s':>15}{'recovery_s':>12}  final"
    )
    for result in results:
        errors = result["errors"]
        print(
            f"{result['name']:<20}{result['requests']:>9}{result['unexpected']:>12}"
            f"{errors['total']:>8}{errors['public']:>8}{errors['deny']:>7}"
            f"{errors['profile']:>9}{result['disturbance_seconds']:>15}"
            f"{result['recovery_seconds']:>12}  {result['final']}"
        )
        if "recovery_by_endpoint_ms" in result:
            print(f"  recovery by endpoint (ms): {json.dumps(result['recovery_by_endpoint_ms'])}")
    print(
        "errors = responses in the disturbed window with no status or status >= 400;"
        " public/deny/profile break it down."
    )
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(results, indent=2) + "\n")
        print(f"Results written to {args.output}")
    print("PASS: every scenario matched its documented behavior")


if __name__ == "__main__":
    main()
