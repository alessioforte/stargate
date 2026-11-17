#!/usr/bin/env python3
"""Measure gateway request paths against disposable local Docker backends.

Starts a single Redis and PostgreSQL container on localhost, runs the ignored
release workload `cluster_request_path_workload` (per-credential and
per-policy Redis command counts plus latency), and writes the results with the
machine, image and commit they belong to.

Both images are pinned by their OCI index digest: every host pulls its own
native platform manifest, so the containers run natively while their content
stays fixed.
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


REDIS_IMAGE = "redis:8.10-alpine@sha256:71885d8cc6e42f2bc0aa2eb853bfc4be200b5108a46ac50e117ae1df0531fb80"
POSTGRES_IMAGE = "postgres:17@sha256:3b0cda93be7c8dba2ed484d19393d93845dc3dafb88973e248ba8e6d99d784e1"


def output(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def ensure_image(reference: str) -> None:
    """Pull the pinned digest only when it is not already local."""
    present = subprocess.run(
        ["docker", "image", "inspect", reference],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if present.returncode != 0:
        subprocess.run(["docker", "pull", reference], check=True)


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
        (REDIS_IMAGE, 6379, []),
        (
            POSTGRES_IMAGE,
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
            ensure_image(image)
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
            redis_port = backends[REDIS_IMAGE]["port"]
            postgres_port = backends[POSTGRES_IMAGE]["port"]
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
