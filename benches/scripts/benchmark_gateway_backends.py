#!/usr/bin/env python3
"""Measure application backend calls against disposable local Docker containers."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import uuid


def output(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--verify", action="store_true",
        help="also run isolated Redis gateway correctness checks",
    )
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    containers: list[str] = []
    backends: dict[str, dict[str, str]] = {}
    metadata = {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "rustc": output("rustc", "-Vv"),
        "commit": output("git", "-C", str(root), "rev-parse", "HEAD"),
    }
    images: list[tuple[str, int, list[str]]] = [
        ("redis:latest", 6379, []),
        ("postgres:17-alpine", 5432, [
            "-e", "POSTGRES_USER=benchmark", "-e", "POSTGRES_PASSWORD=benchmark",
            "-e", "POSTGRES_DB=benchmark",
        ]),
    ]
    try:
        for image, port, options in images:
            name = "stargate-benchmark-" + uuid.uuid4().hex[:12]
            identifier = output(
                "docker", "run", "--pull=never", "--rm", "-d", "--name", name,
                "-p", f"127.0.0.1::{port}", *options, image,
            )
            containers.append(identifier)
            mapped = output("docker", "port", identifier, str(port)).rsplit(":", 1)[1]
            backends[image] = {
                "image_id": output("docker", "inspect", "--format", "{{.Image}}", identifier),
                "port": mapped,
            }
            for _ in range(100):
                if port == 6379:
                    probe = ["redis-cli", "ping"]
                else:
                    # The image starts a temporary Unix-socket server for init;
                    # only the final TCP listener can serve the mapped port.
                    probe = [
                        "pg_isready", "-h", "127.0.0.1", "-U", "benchmark",
                        "-d", "benchmark", "-t", "1",
                    ]
                ready = subprocess.run(
                    ["docker", "exec", identifier, *probe],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                )
                if ready.returncode == 0:
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError(f"{image} did not become ready")
        with tempfile.TemporaryDirectory(prefix="stargate-backend-benchmark-") as directory:
            result = Path(directory) / "results.json"
            redis_port = backends["redis:latest"]["port"]
            postgres_port = backends["postgres:17-alpine"]["port"]
            env = os.environ.copy()
            env.update({
                "STARGATE_BENCH_REDIS_URL": f"redis://127.0.0.1:{redis_port}",
                "STARGATE_BENCH_DATABASE_URL": (
                    f"postgres://benchmark:benchmark@127.0.0.1:{postgres_port}/benchmark"
                ),
                "STARGATE_BENCH_OUTPUT": str(result),
            })
            archives = list((root / "target/debug/build").glob(
                "utoipa-swagger-ui-*/out/v5.17.14.zip"
            ))
            if archives and "SWAGGER_UI_DOWNLOAD_URL" not in env:
                env["SWAGGER_UI_DOWNLOAD_URL"] = archives[0].resolve().as_uri()
            subprocess.run([
                "cargo", "test", "--locked", "--release", "--features", "cluster",
                "cluster_backend_round_trips", "--", "--ignored", "--nocapture",
                "--test-threads=1",
            ], cwd=root, env=env, check=True)
            verification = None
            if args.verify:
                verification_path = Path(directory) / "verification.json"
                env["STARGATE_BENCH_OUTPUT"] = str(verification_path)
                subprocess.run([
                    "cargo", "test", "--locked", "--release", "--features", "cluster",
                    "cluster_gateway_redis_integration", "--", "--ignored", "--nocapture",
                    "--test-threads=1",
                ], cwd=root, env=env, check=True)
                verification = json.loads(verification_path.read_text())
            path = args.output.resolve()
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({
                "metadata": {**metadata, **backends}, "results": json.loads(result.read_text()),
                "verification": verification,
            }, indent=2) + "\n")
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
