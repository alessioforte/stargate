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
from pathlib import Path

from redis_fixtures import write_transactional_templates

ROOT = Path(__file__).resolve().parents[2]
BINARY = ROOT / "target/debug/stargate"

# Store initialization runs before the listener opens, so a missing or
# unreachable Redis stops the process. When startup without Redis becomes
# supported, invert this expectation: the scenario then asserts readiness and
# a Redis component reported unhealthy instead of an early exit.
STARTUP_WITHOUT_REDIS_FAILS = True

# Two routes with the same limit and different failure policies: `/bench/public`
# fails open (200 without allowance headers), `/bench/deny` fails closed with
# `gateway.limiter_unavailable`. `/account/profile` reads the store on every
# request and stays fail-closed.
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
    params: {max_burst: 10000, replenish_1_per: 1ms}
  bench_deny:
    strategy: gcra
    params: {max_burst: 10000, replenish_1_per: 1ms}
http:
  services:
    ok: {kind: direct_response, status: 200}
  policies:
    bench: {kind: rate_limit, limit: bench}
    bench_deny: {kind: rate_limit, limit: bench_deny, on_store_error: deny}
  routers:
    bench:
      match: {path: {exact: /bench/public}}
      service: ok
      policies: [bench]
    bench_deny:
      match: {path: {exact: /bench/deny}}
      service: ok
      policies: [bench_deny]
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
    "profile": ("/account/profile", 20),
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


def request(url, method="GET", body=None, token=None, api_key=None, timeout=5):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if api_key:
        headers["x-api-key"] = api_key
    payload = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=payload, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
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
    def __init__(self):
        self.ids = []

    def run(self, image, port, options=(), command=()):
        # A fixed host port survives stop/start: with an ephemeral mapping Docker
        # Desktop assigns a new host port on restart and the server could not
        # reconnect. The race against another process binding it is negligible.
        host_port = free_port()
        result = docker(
            "run",
            "-d",
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

    def cleanup(self):
        for cid in reversed(self.ids):
            docker("rm", "-f", cid, check=False)
        self.ids.clear()


def redis_ready(cid):
    result = docker("exec", cid, "redis-cli", "PING", check=False)
    return result.returncode == 0 and result.stdout.strip() == "PONG"


def redis_command(cid, *command, input=None):
    return docker("exec", "-i", cid, "redis-cli", *command, input=input, check=False)


def wait_redis(cid, timeout=30):
    wait_until(lambda: redis_ready(cid), timeout, "Redis did not become ready")


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
    def __init__(self, directory, proc, base, username, password, log_handle):
        self.directory = directory
        self.proc = proc
        self.base = base
        self.username = username
        self.password = password
        self.api_key = None
        self.log_handle = log_handle
        self.log_path = directory / "server.log"

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


def start_server(ctx, name, redis_url, bootstrap=True):
    directory = Path(tempfile.mkdtemp(prefix=f"redis-failure-{name}-"))
    write_transactional_templates(directory)
    (directory / "config.yaml").write_text(CONFIG)
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
        OTEL_ENABLED="false",
        SERVER_DRAIN_DELAY_SECS="0",
    )
    require(BINARY.exists(), f"{BINARY} is missing; run cargo build --features cluster")
    if bootstrap:
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
    log = (directory / "server.log").open("w+")
    proc = subprocess.Popen(
        [str(BINARY)],
        env=env,
        cwd=str(directory),
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    server = Server(
        directory, proc, f"http://127.0.0.1:{port}", username, password, log
    )
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
    def __init__(self, base, token, clients=2, interval=0.1):
        self.base = base
        self.token = token
        self.clients = clients
        self.interval = interval
        self.samples = []
        self.lock = threading.Lock()
        self.stopped = threading.Event()
        self.threads = []

    def start(self):
        # One pool per endpoint: a session read blocking on the store timeout
        # must not pause the availability probes.
        for name in ("public", "deny", "profile"):
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
        status, headers, body, error = request(
            self.base + path,
            token=self.token if name == "profile" else None,
            timeout=timeout,
        )
        sample = {
            "name": name,
            "at": started,
            "done": time.monotonic(),
            "status": status,
            "allowance": "x-ratelimit-limit" in headers,
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
    return {
        "public": lambda s: s["status"] == 200 and s["allowance"],
        "deny": lambda s: s["status"] == 200,
        "profile": lambda s: s["status"] == 200,
    }[sample["name"]](sample)


def unavailable_sample(sample):
    return {
        "public": lambda s: s["status"] == 200 and not s["allowance"],
        "deny": lambda s: (
            s["status"] == 503 and s["code"] == "gateway.limiter_unavailable"
        ),
        "profile": lambda s: s["status"] is not None and s["status"] >= 500,
    }[sample["name"]](sample)


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
    # Assertions require a request that both started and finished inside the
    # fault: a store call that outlives it can still succeed.
    during = [sample for sample in disturbed if sample["done"] <= fault_end - end_guard]
    after = between(samples, recovered + start_guard, time.monotonic())
    return before, during, disturbed, after


def count_errors(samples):
    """Failed responses observed while the fault was active.

    `unexpected` checks the phase expectation and stops the run, so it is always
    zero in a passing report. This counts the responses that failed during the
    disturbance: no status (client timeout) or `>= 400`.
    """
    errors = {"total": 0, "public": 0, "deny": 0, "profile": 0}
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

    time.sleep(1)
    docker("start", redis)
    fault_end = time.monotonic()
    wait_redis(redis)
    wait_until(settled(server, token), 30, "limits and sessions did not recover")
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
    return finish(
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


def scenario_empty(ctx):
    redis, server, token, refresh = start_cluster(
        ctx, "empty", ["--save", "", "--appendonly", "no"]
    )
    sid = json.loads(_b64decode(token.split(".")[1]))["sid"]
    require(
        redis_command(redis, "EXISTS", sid).stdout.strip() == "1",
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

    time.sleep(1)
    docker("start", redis)
    fault_end = time.monotonic()
    wait_redis(redis)
    require(
        redis_command(redis, "EXISTS", sid).stdout.strip() == "0",
        "the restarted Redis still holds the session",
    )
    status, _, body, _ = request(server.url("/account/profile"), token=token)
    require(
        status == 401 and body["code"] == "auth.token_invalid",
        f"stale session profile: {status} {body and body.get('code')}",
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
    return finish(
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


def scenario_partition(ctx):
    redis, redis_port = ctx.containers.run(ctx.args.redis_image, 6379, [])
    wait_redis(redis)
    relay_port = free_port()
    relay = subprocess.Popen(
        [sys.executable, "-c", RELAY, str(relay_port), str(redis_port)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        wait_until(lambda: _port_open(relay_port), 10, "the Redis relay did not start")
        server = start_server(ctx, "partition", f"redis://127.0.0.1:{relay_port}")
        server.wait_ready()
        token, _ = server.login()
        server.create_admin_key(token)

        load = Load(server.base, token)
        load.start()
        time.sleep(0.6)
        baseline_start = time.monotonic()
        time.sleep(0.6)

        fault_start = time.monotonic()
        os.kill(relay.pid, signal.SIGSTOP)
        time.sleep(1)
        status, headers, _, _ = request(server.url("/bench/public"))
        require(status == 200, f"public route during partition: {status}")
        require(
            "x-ratelimit-limit" not in headers, "fail-open emitted allowance headers"
        )
        status, _, body, _ = request(server.url("/bench/deny"))
        require(
            status == 503 and body["code"] == "gateway.limiter_unavailable",
            f"deny route during partition: {status} {body}",
        )
        health = server.health()
        require(health["redis"]["status"] == "unhealthy", "health missed the partition")
        require(health["status"] == "unhealthy", "overall health missed the partition")
        require(
            health["limits"]["storeDegraded"], "health missed the degraded limit checks"
        )
        require(request(server.url("/readyz"))[0] == 200, "readiness followed Redis")
        # A store read waits for the 5s response timeout, then fails closed.
        status, _, _, _ = request(server.url("/account/profile"), token=token, timeout=20)
        require(
            status is not None and status >= 500,
            f"profile did not fail closed during the partition: {status}",
        )

        fault_end = time.monotonic()
        os.kill(relay.pid, signal.SIGCONT)
        wait_until(settled(server, token), 30, "limits and sessions did not recover")
        recovered = time.monotonic()
        require(server.proc.poll() is None, "Stargate restarted during the partition")
        time.sleep(0.4)
        load.stop()
        server.stop()
    finally:
        relay.terminate()
        try:
            relay.wait(timeout=10)
        except subprocess.TimeoutExpired:
            relay.kill()
            relay.wait()

    # Only requests that finished before the partition was lifted belong to the
    # disturbed window; their store timeout expired while the fault was active.
    before, during, disturbed, after = windows(
        load.snapshot(), baseline_start, fault_start, fault_end, recovered, 0.3, 0.3
    )
    for window, label in ((before, "before"), (during, "during"), (after, "after")):
        require_samples(window, label)
    return finish(
        "partition",
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
    redis, port = ctx.containers.run(ctx.args.redis_image, 6379, [])
    wait_redis(redis)
    ctx.containers.remove(redis)

    server = start_server(ctx, "absent", f"redis://127.0.0.1:{port}", bootstrap=False)
    started = time.monotonic()
    if STARTUP_WITHOUT_REDIS_FAILS:
        try:
            server.proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            raise AssertionError(
                "the server kept running without Redis:\n" + server.redacted_log()
            )
        elapsed = time.monotonic() - started
        log = server.redacted_log()
        status, _, _, _ = request(server.url("/readyz"), timeout=2)
        require(status is None, f"the server answered HTTP without Redis: {status}")
        require(
            server.proc.returncode != 0,
            f"startup without Redis exited {server.proc.returncode}: {log}",
        )
        require(
            "Redis" in log or "redis" in log,
            "startup failure does not name Redis:\n" + log,
        )
        final = "failed at startup"
    else:
        # Inverse expectation: the process starts, serves readiness and reports
        # Redis as unhealthy. Add the admin key and health assertion here.
        server.wait_ready(timeout=60)
        elapsed = time.monotonic() - started
        final = "ready with Redis unavailable"
    return {
        "name": "absent-at-startup",
        "requests": 0,
        "unexpected": 0,
        "errors": {"total": 0, "public": 0, "deny": 0, "profile": 0},
        "disturbance_seconds": round(elapsed, 2),
        "recovery_seconds": 0,
        "final": final,
    }


def _b64decode(value):
    return base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))


def _port_open(port):
    with socket.socket() as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


class Context:
    def __init__(self, args):
        self.args = args
        self.containers = Containers()
        self.servers = []
        self.postgres_port = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--redis-image", required=True)
    parser.add_argument("--postgres-image", required=True)
    parser.add_argument(
        "--scenarios",
        default="restart,empty,partition,memory-full,absent-at-startup",
        help="comma-separated scenario names, all by default",
    )
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    scenarios = {
        "restart": scenario_restart,
        "empty": scenario_empty,
        "partition": scenario_partition,
        "memory-full": scenario_memory_full,
        "absent-at-startup": scenario_absent,
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
