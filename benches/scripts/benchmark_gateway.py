#!/usr/bin/env python3
"""Run isolated release gateway workloads with public test signing material."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=3)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument(
        "--scenarios", help="comma-separated scenario names; omit for the complete matrix"
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--verify", action="store_true",
        help="run combined lifecycle and metrics verification",
    )
    args = parser.parse_args()
    if args.seconds < 1 or args.runs < 1:
        parser.error("seconds and runs must be positive")
    if args.verify and args.scenarios:
        parser.error("--verify cannot be combined with --scenarios")
    root = Path(__file__).resolve().parents[2]
    args.output = args.output.resolve()
    fixtures = root / "crates/ctx/tests/fixtures"
    metadata = {
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "rustc": command("rustc", "-Vv"),
        "commit": command("git", "-C", str(root), "rev-parse", "HEAD"),
        "seconds_per_case": args.seconds,
        "runs": args.runs,
    }
    if platform.system() == "Darwin":
        metadata["hardware"] = {
            key: command("sysctl", "-n", key)
            for key in [
                "hw.model", "hw.memsize", "hw.physicalcpu", "hw.logicalcpu",
                "machdep.cpu.brand_string",
            ]
        }
    with tempfile.TemporaryDirectory(prefix="stargate-benchmark-") as directory:
        directory = Path(directory)
        jwks = directory / "jwks.json"
        jwks.write_text(json.dumps({
            "keys": [json.loads((fixtures / "public.jwk.json").read_text())]
        }))
        env = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("JWT_", "INTERNAL_CONTEXT_", "OTEL_"))
        }
        env.update({
            "JWT_ALGORITHM": "HS256",
            "JWT_SECRET": "public-benchmark-secret-used-only-for-local-fixtures",
            "JWT_ISSUER": "http://benchmark.invalid",
            "INTERNAL_CONTEXT_ALGORITHM": "RS256",
            "INTERNAL_CONTEXT_ISSUER": "https://benchmark.invalid/internal-context",
            "INTERNAL_CONTEXT_KID": "stargate-internal-test",
            "INTERNAL_CONTEXT_PRIVATE_KEY_PATH": str(fixtures / "private.pem"),
            "INTERNAL_CONTEXT_JWKS_PATH": str(jwks),
            "INTERNAL_CONTEXT_SIGNING_WORKERS": "2",
            "INTERNAL_CONTEXT_SIGNING_QUEUE_CAPACITY": "32",
            "INTERNAL_CONTEXT_SIGNING_QUEUE_TIMEOUT_MS": "50",
            "STARGATE_BENCH_SECONDS": str(args.seconds),
        })
        env.pop("STARGATE_BENCH_SCENARIOS", None)
        if args.scenarios:
            env["STARGATE_BENCH_SCENARIOS"] = args.scenarios
        # The pinned Swagger archive can be reused when the build has no network.
        archives = list((root / "target/debug/build").glob(
            "utoipa-swagger-ui-*/out/v5.17.14.zip"
        ))
        if archives and "SWAGGER_UI_DOWNLOAD_URL" not in env:
            env["SWAGGER_UI_DOWNLOAD_URL"] = archives[0].resolve().as_uri()
        runs = []
        for index in range(args.runs):
            result = directory / f"run-{index}.json"
            env["STARGATE_BENCH_OUTPUT"] = str(result)
            subprocess.run([
                "cargo", "test", "--locked", "--release", "--features", "edge",
                "gateway_combined_verification" if args.verify else "gateway_release_workloads",
                "--", "--ignored", "--nocapture",
                "--test-threads=1",
            ], cwd=root, env=env, check=True)
            runs.append(json.loads(result.read_text()))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({
            "metadata": metadata, "runs": runs,
        }, indent=2) + "\n")
        print(f"Results written to {args.output}")


if __name__ == "__main__":
    main()
