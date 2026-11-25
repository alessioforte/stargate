#!/usr/bin/env python3
"""Verify DATA_PATH follows the edge data mount, stays clear of the read-only mounts, and seeds stay read-only."""

import json
import subprocess
import tempfile
import unittest
from pathlib import Path

CHART = Path(__file__).resolve().parents[1]


def render(overrides=None, *, cluster=False):
    with tempfile.TemporaryDirectory(prefix="stargate-data-path-test-") as directory:
        command = ["helm", "template", "check", str(CHART)]
        if cluster:
            command += ["-f", str(CHART / "values-cluster.yaml")]
        if overrides:
            values = Path(directory) / "values.json"
            values.write_text(json.dumps(overrides), encoding="utf-8")
            command += ["-f", str(values)]
        return subprocess.run(command, capture_output=True, text=True, check=False)


def documents(manifest):
    command = ["ruby", "-ryaml", "-rjson", "-e", "puts JSON.generate(YAML.load_stream(STDIN.read).compact)"]
    result = subprocess.run(command, input=manifest, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)


def resource(items, kind, name):
    return next(item for item in items if item["kind"] == kind and item["metadata"]["name"] == name)


def container(item):
    return item["spec"]["template"]["spec"]["containers"][0]


def env_value(workload, name):
    return next(entry["value"] for entry in workload["env"] if entry["name"] == name)


def mounts(workload):
    return [(mount["name"], mount["mountPath"]) for mount in workload.get("volumeMounts", [])]


class EdgeDataPath(unittest.TestCase):
    def test_default_edge_data_path_uses_the_claim(self):
        result = render()
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        deployment = container(resource(items, "Deployment", "check-stargate"))
        self.assertEqual(env_value(deployment, "DATA_PATH"), "/app/.stargate")
        self.assertIn(("data", "/app/.stargate"), mounts(deployment))
        gateway = next(mount for mount in deployment["volumeMounts"] if mount["name"] == "gateway-config")
        self.assertEqual(gateway["mountPath"], "/etc/stargate")
        self.assertTrue(gateway["readOnly"])

    def test_custom_mount_path_drives_data_path_and_keeps_bundled_assets(self):
        result = render({
            "persistence": {"mountPath": "/app/data"},
            "bootstrap": {
                "enabled": True,
                "email": "admin@example.com",
                "existingSecret": "admin-password",
                "oauthRedirectUri": "https://identity.example.com/stargate/auth/callback",
            },
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        items = documents(result.stdout)
        deployment = container(resource(items, "Deployment", "check-stargate"))
        self.assertEqual(env_value(deployment, "DATA_PATH"), "/app/data")
        self.assertIn(("data", "/app/data"), mounts(deployment))
        self.assertIn(("data", "/app/.stargate"), mounts(deployment))
        gateway = next(mount for mount in deployment["volumeMounts"] if mount["name"] == "gateway-config")
        self.assertEqual(gateway["mountPath"], "/etc/stargate")
        self.assertTrue(gateway["readOnly"])

        bootstrap = container(resource(items, "Job", "check-stargate-bootstrap"))
        self.assertEqual(env_value(bootstrap, "DATA_PATH"), "/app/data")
        self.assertIn(("data", "/app/data"), mounts(bootstrap))

    def test_cluster_rendering_has_no_data_path(self):
        result = render(cluster=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        deployment = container(resource(documents(result.stdout), "Deployment", "check-stargate"))
        self.assertFalse(any(entry["name"] == "DATA_PATH" for entry in deployment["env"]))

    def test_data_path_is_chart_managed_in_edge_mode(self):
        for overrides, path in [
            ({"runtimeConfig": {"data": {"DATA_PATH": "/tmp/data"}}}, "runtimeConfig.data"),
            ({"extraEnv": [{"name": "DATA_PATH", "value": "/tmp/data"}]}, "extraEnv"),
        ]:
            with self.subTest(path=path):
                result = render(overrides)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"{path}.DATA_PATH is chart-managed", result.stderr)

    def test_data_mount_must_not_overlap_the_seed_mount(self):
        for mount in ["/etc/stargate", "/etc/stargate/data", "/etc"]:
            with self.subTest(mount=mount):
                result = render({"persistence": {"mountPath": mount}})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    f'persistence.mountPath "{mount}" must not be equal to, nested in, or a parent of '
                    'gatewayConfig.mountPath "/etc/stargate"',
                    result.stderr,
                )

    def test_data_mount_must_not_overlap_the_internal_context_mount(self):
        for mount in ["/run/internal-context", "/run/internal-context/data", "/run"]:
            with self.subTest(mount=mount):
                result = render({
                    "internalContext": {
                        "enabled": True,
                        "existingSecret": "internal-context",
                        "mountPath": "/run/internal-context",
                        "issuer": "https://stargate.internal",
                        "kid": "stargate-internal-current",
                    },
                    "persistence": {"mountPath": mount},
                })
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    f'persistence.mountPath "{mount}" must not be equal to, nested in, or a parent of '
                    'internalContext.mountPath "/run/internal-context"',
                    result.stderr,
                )


if __name__ == "__main__":
    unittest.main()
