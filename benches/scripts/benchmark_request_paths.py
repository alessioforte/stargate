#!/usr/bin/env python3
"""Measure gateway request paths against disposable local Docker backends.

Starts a single Redis and PostgreSQL container on localhost, runs the ignored
release workload `cluster_request_path_workload` (per-credential and
per-policy Redis command counts plus latency), and writes the results with the
machine, image and commit they belong to.
"""

import argparse
import json
import os
import platform
import secrets
import subprocess
import tempfile
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path


def output(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def probe(container: str, command: list[str]) -> bool:
    return (
        subprocess.run(
            ["docker", "exec", container, *command],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    )


def machine() -> dict[str, str]:
    fields = {
        "cpu": ("sysctl", "-n", "machdep.cpu.brand_string"),
        "cpus": ("sysctl", "-n", "hw.ncpu"),
        "memory_bytes": ("sysctl", "-n", "hw.memsize"),
        "os": ("sw_vers", "-productVersion"),
    }
    details = {}
    for name, command in fields.items():
        try:
            details[name] = output(*command)
        except (OSError, subprocess.CalledProcessError):
            details[name] = "unknown"
    return details


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=4)
    parser.add_argument("--workers", type=int, default=16)
    parser.add_argument(
        "--scenarios",
        default="",
        help="comma-separated subset: anonymous,api_key,session,oauth,rate_limit,quota",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    containers: list[str] = []
    backends: dict[str, dict[str, str]] = {}
    metadata = {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "machine": machine(),
        "rustc": output("rustc", "-Vv").splitlines()[0],
        "commit": output("git", "-C", str(root), "rev-parse", "HEAD"),
    }
    images: list[tuple[str, int, list[str]]] = [
        ("redis:latest", 6379, []),
        (
            "postgres:17-alpine",
            5432,
            [
                "-e",
                "POSTGRES_USER=benchmark",
                "-e",
                "POSTGRES_PASSWORD=benchmark",
                "-e",
                "POSTGRES_DB=benchmark",
            ],
        ),
    ]
    try:
        for image, port, options in images:
            name = "stargate-request-paths-" + uuid.uuid4().hex[:12]
            identifier = output(
                "docker",
                "run",
                "--pull=never",
                "--rm",
                "-d",
                "--name",
                name,
                "-p",
                f"127.0.0.1::{port}",
                *options,
                image,
            )
            containers.append(identifier)
            mapped = output("docker", "port", identifier, str(port)).rsplit(":", 1)[1]
            backends[image] = {
                "image_id": output(
                    "docker", "inspect", "--format", "{{.Image}}", identifier
                ),
                "port": mapped,
            }
            if port == 6379:
                ready = ["redis-cli", "ping"]
            else:
                ready = [
                    "pg_isready",
                    "-h",
                    "127.0.0.1",
                    "-U",
                    "benchmark",
                    "-d",
                    "benchmark",
                    "-t",
                    "1",
                ]
            for _ in range(100):
                if probe(identifier, ready):
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"{image} did not become ready")
        with tempfile.TemporaryDirectory(prefix="stargate-request-paths-") as directory:
            result = Path(directory) / "results.json"
            redis_port = backends["redis:latest"]["port"]
            postgres_port = backends["postgres:17-alpine"]["port"]
            env = os.environ.copy()
            env.update(
                {
                    "REDIS_URL": f"redis://127.0.0.1:{redis_port}",
                    "REDIS_KEY_PREFIX": "",
                    "POSTGRES_ENDPOINT": f"127.0.0.1:{postgres_port}",
                    "POSTGRES_USERNAME": "benchmark",
                    "POSTGRES_PASSWORD": "benchmark",
                    "POSTGRES_DATABASE": "benchmark",
                    "POSTGRES_SSLMODE": "disable",
                    "POSTGRES_CREATE_DATABASE": "false",
                    "DB_MIGRATE_ON_START": "true",
                    "JWT_ALGORITHM": "HS256",
                    "JWT_SECRET": secrets.token_hex(32),
                    "STARGATE_BENCH_SECONDS": str(args.seconds),
                    "STARGATE_BENCH_WORKERS": str(args.workers),
                    "STARGATE_BENCH_OUTPUT": str(result),
                    "OTEL_ENABLED": "false",
                    "STARGATE_RUNTIME_PROFILE": "cluster",
                }
            )
            if args.scenarios:
                env["STARGATE_BENCH_SCENARIOS"] = args.scenarios
            subprocess.run(
                [
                    "cargo",
                    "test",
                    "--locked",
                    "--release",
                    "--features",
                    "cluster",
                    "cluster_request_path_workload",
                    "--",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ],
                cwd=root,
                env=env,
                check=True,
            )
            path = args.output.resolve()
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                json.dumps(
                    {
                        "metadata": {**metadata, "backends": backends},
                        "results": json.loads(result.read_text()),
                    },
                    indent=2,
                )
                + "\n"
            )
            print(f"Results written to {path}")
    finally:
        failures = []
        for identifier in reversed(containers):
            try:
                stopped = subprocess.run(
                    ["docker", "stop", "--time", "1", identifier],
                    stdout=subprocess.DEVNULL,
                )
                if stopped.returncode:
                    failures.append(identifier)
            except OSError:
                failures.append(identifier)
        if failures:
            raise RuntimeError(f"Unable to stop benchmark containers: {failures}")


if __name__ == "__main__":
    main()
